// One part of the engine's tests (src/render/tests.rs includes it): brush groups, dab streams under turns, the kernel, flow and accumulation laws, and the raster size policy.

/// R27 Batch-4 (L-08) → **R29 Batch-6b: a carried brush group draws its
/// DABS.** Rewritten, not deleted: this test was mutation-lined against
/// exactly this change, and what it pins now is the other side of it.
///
/// Until R29 Batch-6b `mask_weight`'s `Brush` arm was the literal `=> 0.0`
/// and this test asserted that zero at six points. The zero was honest
/// while the alpha kernel was unmeasured; R29 Batch-6 measured it (29
/// controlled Lightroom exports), so the zero became the invention it had
/// been guarding against.
///
/// MUTATION-LINED, in both directions:
///  * restoring `=> 0.0` fails the「paints」asserts;
///  * answering `1.0` (the "just treat it as fully painted" shortcut) fails
///    the far-corner asserts;
///  * dropping `brush_raster` from `apply_masks`/`mask_coverage` leaves the
///    `bmp` slot `None`, which fails the same「paints」asserts.
#[test]
fn a_carried_brush_group_draws_its_dabs() {
    // `P12` Mask 7 -> Brush 1, stroke 1: two dabs near the bottom-left
    // corner, radius 0.5818 in WIDTH units, density 0.4398.
    let g = probe_brush(&[(
        0.439815,
        0.582157,
        1.0,
        0.0,
        "r 0.581835\nd 0.100684 0.840004\nr 0.581172\nd 0.213862 0.887261",
    )]);
    let (fw, fh) = (480u32, 320u32);
    let raster = brush_raster(&g, fw, fh).expect("a group with two dabs rasterises");
    // ON the dabs it paints; three frame-widths away it does not. Both
    // halves matter: the first was `0.0` before this batch, and the second
    // is what a blanket `1.0` would break.
    for (nx, ny) in [(0.100684f32, 0.840004f32), (0.213862, 0.887261)] {
        let w = mask_weight(&g, nx, ny, Some(&raster));
        assert!(w > 0.3, "a brush must paint on its own dab at ({nx}, {ny}): {w}");
    }
    for (nx, ny) in [(0.0f32, 0.0f32), (0.99, 0.02)] {
        let w = mask_weight(&g, nx, ny, Some(&raster));
        assert!(w < 0.02, "and nothing at ({nx}, {ny}), ρ > 1 from every dab: {w}");
    }
    // The raster is a RENDER-time artefact, so `mask_weight` still means
    // "the weight of this geometry at this STORED point" and answers the
    // inert 0 when no alpha was built — which is also the contract every
    // other test in this file asserts `mask_weight` against.
    assert_eq!(
        mask_weight(&g, 0.100684, 0.840004, None),
        0.0,
        "no alpha in hand = inert, never a guess"
    );
    // And since R29 C1 the migration treats it like every other geometry:
    // a brush group is a TURNABLE coordinate, not a raster the disclosure
    // has to apologise for. (The algebra itself is
    // `a_quarter_turn_rewrites_the_dab_stream_and_rescales_its_radii`.)
    use crate::recipe::LocalAdjustment;
    let mut r = EditRecipe {
        masks: vec![LocalAdjustment { mask: g, ..Default::default() }],
        ..Default::default()
    };
    assert!(recipe_has_frame_coords(&r), "and its dabs ARE frame coordinates");
    let before = r.clone();
    orient_recipe_coords(&mut r, Orientation::Rotate270, probe_frame());
    assert_ne!(r, before, "a quarter turn must move the dab stream");
}

/// The dab stream of the first stroke of the first mask — the thing every
/// R29 C1 assertion below is about.
fn only_stream(r: &EditRecipe) -> &str {
    let MaskGeometry::Brush { strokes, .. } = &r.masks[0].mask else { panic!("a brush") };
    &strokes[0].dabs
}

/// …and its `crs:Radius`, the stream's initial state.
fn only_radius(r: &EditRecipe) -> f32 {
    let MaskGeometry::Brush { strokes, .. } = &r.masks[0].mask else { panic!("a brush") };
    strokes[0].radius
}

/// A one-mask recipe around [`probe_brush`], with `radius` on the stroke.
fn brushed_recipe(radius: f32, dabs: &str) -> EditRecipe {
    use crate::recipe::LocalAdjustment;
    EditRecipe {
        masks: vec![LocalAdjustment {
            mask: probe_brush(&[(1.0, radius, 1.0, 0.0, dabs)]),
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// R29 C1 (the 2026-08-21 ruling) — a turn REWRITES the dab stream instead
/// of carrying it. Three independent claims, each a different way to get it
/// wrong:
///
///  * a `d` token moves through [`orient_point`], the same function the
///    crop corners and the radial box beside it use;
///  * an `r` token AND `BrushStroke::radius` are rescaled by the frame
///    aspect, but ONLY for the four states that exchange the axes —
///    `crs:Radius` is in width units while a dab is a circle in pixels, so
///    a quarter turn has to exchange the unit too, and a half turn must
///    not;
///  * `f` (flow) and `h` (hardness) are deposit laws, not positions, and
///    are never touched by anything.
///
/// The expected TEXT is hand-derived from `orient_point`'s own table
/// against the A7R IV's 3:2 frame, so this asserts the output rather than a
/// re-derivation of it. The six-decimal form is Lightroom's own
/// ([`LR_DAB_DECIMALS`]).
///
/// MUTATIONS THIS CATCHES:
///  * turning `(x, y)` by anything but `orient_point(o, …)` (e.g. the
///    inverse state, or `(y, x)`) — the `Rotate90`/`Rotate270` rows;
///  * dropping the radius rescale (`radius_scale` pinned at 1.0) — the
///    `r 0.300000` and `0.375` asserts;
///  * applying it on every state instead of the transposing four — the
///    `Rotate180` row, which must keep `r 0.200000` and `0.25`;
///  * rescaling `f`/`h` along with `r` — the `f 0.500000` / `h 1.000000`
///    asserts.
#[test]
fn a_quarter_turn_rewrites_the_dab_stream_and_rescales_its_radii() {
    const SEED: &str =
        "r 0.200000\nf 0.500000\nh 1.000000\nd 0.100000 0.800000\nd 0.300000 0.400000";
    for (o, want_stream, want_radius) in [
        // (u,v) -> (1-v, u); the axes swap, so every radius takes W/H = 1.5.
        (
            Orientation::Rotate90,
            "r 0.300000\nf 0.500000\nh 1.000000\nd 0.200000 0.100000\nd 0.600000 0.300000",
            0.375f32,
        ),
        // (u,v) -> (1-u, 1-v); the frame keeps its shape, so no radius moves.
        (
            Orientation::Rotate180,
            "r 0.200000\nf 0.500000\nh 1.000000\nd 0.900000 0.200000\nd 0.700000 0.600000",
            0.25,
        ),
        // (u,v) -> (v, 1-u); axes swap again.
        (
            Orientation::Rotate270,
            "r 0.300000\nf 0.500000\nh 1.000000\nd 0.800000 0.900000\nd 0.400000 0.700000",
            0.375,
        ),
    ] {
        let mut r = brushed_recipe(0.25, SEED);
        assert!(orient_recipe_coords(&mut r, o, probe_frame()));
        assert_eq!(only_stream(&r), want_stream, "{o:?}: the rewritten stream");
        assert!(
            (only_radius(&r) - want_radius).abs() < 1e-6,
            "{o:?}: crs:Radius is {} , not {want_radius}",
            only_radius(&r)
        );
    }

    // FOUR quarter turns are the identity — and the frame handed in has to
    // turn with them, because after the first one the photo is 6336 × 9504.
    // The radius alternates 0.25 / 0.375 and comes home.
    let mut r = brushed_recipe(0.25, SEED);
    let portrait = CoordFrame::new(6336.0, 9504.0);
    for k in 0..4 {
        let frame = if k % 2 == 0 { probe_frame() } else { portrait };
        assert!(orient_recipe_coords(&mut r, Orientation::Rotate90, frame));
    }
    assert!(
        (only_radius(&r) - 0.25).abs() < 1e-6,
        "a full circle moved the radius: {}",
        only_radius(&r)
    );
    // Exact TEXT equality is not claimed — six decimals is a grid, and four
    // trips over it are four roundings — so the tokens are compared as
    // numbers. On this frame they do in fact come back byte-identical; the
    // tolerance is what the claim is worth, not what today's build does.
    for (got, want) in only_stream(&r).split('\n').zip(SEED.split('\n')) {
        let nums = |t: &str| {
            t.split_whitespace().skip(1).map(|v| v.parse::<f32>().unwrap()).collect::<Vec<_>>()
        };
        assert_eq!(got.split_whitespace().next(), want.split_whitespace().next());
        for (a, b) in nums(got).into_iter().zip(nums(want)) {
            assert!((a - b).abs() < 1e-5, "a full circle moved a token: {got} vs {want}");
        }
    }
}

/// The other side of the same ruling: a photo that is NOT turned keeps its
/// dab stream byte for byte — the identity orientations return before the
/// rewrite can reach a formatter.
///
/// The fixture stream is deliberately NOT in Lightroom's six-decimal form
/// (`d 0 0`, `r .5`): if the identity arm ever went through
/// `turn_brush_strokes`, those would come back as `d 0.000000 0.000000` and
/// `r 0.500000` — legal, identical in value, and a silent rewrite of a
/// file the photographer never rotated.
///
/// The second half pins the honest `None` arm: a caller that cannot supply
/// the frame leaves the stream alone rather than guessing an aspect, and
/// everything else in the recipe still moves.
///
/// MUTATIONS THIS CATCHES: drop the `Normal | Unknown` early return;
/// rewrite the stream unconditionally in the `Brush` arm instead of under
/// `if let Some(f) = frame`.
#[test]
fn an_unturned_photo_keeps_every_dab_byte() {
    const RAW_FORM: &str = "r .5\nd 0 0\nd 1 1";
    for o in [Orientation::Normal, Orientation::Unknown] {
        let mut r = brushed_recipe(0.25, RAW_FORM);
        let before = r.clone();
        assert!(!orient_recipe_coords(&mut r, o, probe_frame()), "{o:?} must report no move");
        assert_eq!(r, before, "{o:?} must not touch a single dab byte");
    }
    // No frame in hand: the dabs stay put, and the migration says so by
    // leaving them rather than by inventing an aspect.
    let mut r = brushed_recipe(0.25, RAW_FORM);
    r.crop = Some(Crop { left: 0.1, top: 0.2, right: 0.8, bottom: 0.9 });
    assert!(orient_recipe_coords(&mut r, Orientation::Rotate90, None));
    assert_eq!(only_stream(&r), RAW_FORM, "no frame, no rewrite");
    assert!((only_radius(&r) - 0.25).abs() < 1e-9, "and no rescale either");
    assert!(r.crop.unwrap().left != 0.1, "while the aspect-free geometry still turned");
}

/// SAME FRAME, checked against a parametric shape rather than against the
/// derivation: a brush dab and a radial centred on the same point must
/// still be centred on the same point after the turn.
///
/// This is what「渲染永远正确」means operationally — the dab stream is not
/// merely moved, it is moved by the ONE map every other geometry in the
/// recipe uses, so a photographer's brush stroke and the gradient they
/// aligned it with do not drift apart on a rotate.
///
/// MUTATION THIS CATCHES: turn the dabs with the INVERSE orientation (a
/// plausible sign slip, since `in_source_frame` really does hand this
/// function an inverse) — the radial still lands correctly and the dab does
/// not, which no test that only looks at the brush could see.
#[test]
fn a_turned_dab_lands_where_the_turned_radial_beside_it_does() {
    use crate::recipe::LocalAdjustment;
    let (px, py) = (0.30f32, 0.65f32);
    // A radial whose box is centred on the dab. Half-extents differ so the
    // centre is not recoverable by accident from a symmetric box.
    let radial = MaskGeometry::Radial {
        top: py - 0.05,
        left: px - 0.12,
        bottom: py + 0.05,
        right: px + 0.12,
        feather: 0.5,
        roundness: 0.0,
        flipped: false,
        angle: 0.0,
        midpoint: 50.0,
        mask_version: 2,
    };
    for o in [
        Orientation::Rotate90,
        Orientation::Rotate180,
        Orientation::Rotate270,
        Orientation::Transpose,
        Orientation::HorizontalFlip,
    ] {
        let mut r = EditRecipe {
            masks: vec![
                LocalAdjustment {
                    mask: probe_brush(&[(1.0, 0.1, 1.0, 0.0, &format!("d {px} {py}"))]),
                    ..Default::default()
                },
                LocalAdjustment { mask: radial.clone(), ..Default::default() },
            ],
            ..Default::default()
        };
        assert!(orient_recipe_coords(&mut r, o, probe_frame()));
        let MaskGeometry::Brush { strokes, .. } = &r.masks[0].mask else { panic!("a brush") };
        let dab: Vec<f32> = strokes[0]
            .dabs
            .split_whitespace()
            .skip(1)
            .map(|v| v.parse::<f32>().unwrap())
            .collect();
        let MaskGeometry::Radial { top, left, bottom, right, .. } = r.masks[1].mask else {
            panic!("a radial")
        };
        let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
        assert!(
            (dab[0] - cx).abs() < 1e-6 && (dab[1] - cy).abs() < 1e-6,
            "{o:?}: the dab landed at ({}, {}) and the radial at ({cx}, {cy})",
            dab[0],
            dab[1]
        );
    }
}

/// The RENDER, which is the claim the ruling actually bought: after a
/// quarter turn a dab is still a CIRCLE in pixels, not an ellipse.
///
/// Radius is the whole reason this batch needed a new input. A dab of
/// `r = 0.1` on a 480 × 320 frame is 48 px across both axes; turn the photo
/// and the frame is 320 × 480, so the SAME 48 px is `r = 0.15` in width
/// units. Sampling the alpha 40 px from the centre along each axis is what
/// separates the two readings: with the rescale both samples sit inside the
/// disc and agree, without it the x-extent has shrunk to 32 px and the
/// horizontal sample falls outside the dab entirely.
///
/// MUTATION THIS CATCHES: pin `CoordFrame::brush_radius_scale` at 1.0. The
/// coordinate half of the migration stays perfect and the mask still draws
/// — in the wrong shape, which is exactly the failure a coordinates-only
/// fix would have shipped.
#[test]
fn a_turned_dab_is_still_a_circle_in_pixels() {
    let (fw, fh) = (480u32, 320u32);
    let mut r = brushed_recipe(0.1, "h 1.000000\nd 0.500000 0.500000");
    let before = brush_raster(&r.masks[0].mask, fw, fh).expect("one dab");
    let flat = |g: &MaskGeometry, ras: &image::GrayImage, w: u32, h: u32| {
        // 40 px from the centre along each axis, in that frame's own
        // normalised coordinates.
        let x = mask_weight(g, 0.5 + 40.0 / w as f32, 0.5, Some(ras));
        let y = mask_weight(g, 0.5, 0.5 + 40.0 / h as f32, Some(ras));
        (x, y)
    };
    let (bx, by) = flat(&r.masks[0].mask, &before, fw, fh);
    assert!(bx > 0.5 && (bx - by).abs() < 0.05, "premise: the dab starts round ({bx}, {by})");

    assert!(orient_recipe_coords(&mut r, Orientation::Rotate90, CoordFrame::new(480.0, 320.0)));
    let after = brush_raster(&r.masks[0].mask, fh, fw).expect("still one dab");
    let (ax, ay) = flat(&r.masks[0].mask, &after, fh, fw);
    assert!(
        ax > 0.5 && (ax - ay).abs() < 0.05,
        "the turned dab must still be round ({ax}, {ay})"
    );
    assert!(
        (ax - bx).abs() < 0.05 && (ay - by).abs() < 0.05,
        "and the same size: was ({bx}, {by}), now ({ax}, {ay})"
    );
}

/// The WIRING, end to end: `apply_develop` really hands the brush arm a
/// raster, and `mask_coverage` really advertises the same one.
///
/// Every other brush test in this file builds the alpha itself and passes
/// it to `mask_weight`, which pins the MODEL and would stay green if the
/// two production call sites forgot to ask for it. This is the test that
/// fails when they do — and「forgot to ask」is exactly the shape the arm's
/// old `=> 0.0` had.
///
/// MUTATION-LINED: dropping either `brush_raster` call — the one in
/// `apply_masks` or the one in `mask_coverage` — turns one of the two
/// halves below into the frame it started from.
#[test]
fn the_develop_hands_the_brush_arm_its_raster() {
    use crate::recipe::LocalAdjustment;
    let (w, h) = (240usize, 160usize);
    // Hard dab (h = 1) at the frame centre, radius 0.2 of the width, at
    // −3 EV: the centre must go dark and the corner must not move at all.
    let mask = LocalAdjustment {
        mask: probe_brush(&[(1.0, 0.2, 1.0, 1.0, "d 0.5 0.5")]),
        exposure_ev: -3.0,
        ..Default::default()
    };
    let r = EditRecipe { masks: vec![mask.clone()], ..Default::default() };
    let mut data = vec![[0.5f32; 3]; w * h];
    apply_develop_anon(&mut data, w, h, &r);
    let at = |x: usize, y: usize| data[y * w + x][1];
    assert!(at(w / 2, h / 2) < 0.2, "the dab centre must darken: {}", at(w / 2, h / 2));
    assert!(
        (at(2, 2) - 0.5).abs() < 1e-6,
        "and the far corner must not move: {}",
        at(2, 2)
    );
    // The GUI's red wash reads the SAME alpha through a different call
    // site, so it gets its own half of the assertion (the overlay agreeing
    // with the render is `the_gui_coverage_overlay_matches_what_the_render_applies`).
    let base = DynamicImage::ImageRgb8(RgbImage::from_pixel(
        w as u32,
        h as u32,
        image::Rgb([128, 128, 128]),
    ));
    let cov = mask_coverage(&mask, &base, MaskFrame::AsRendered);
    assert!(
        cov.get_pixel(w as u32 / 2, h as u32 / 2)[0] > 200,
        "the overlay must show the coverage the render applied"
    );
    assert_eq!(cov.get_pixel(2, 2)[0], 0, "and none where the render applied none");
}

/// The kernel closed form against the MEASURED nine-rung table.
///
/// Provenance for every number below: R29 Batch-6 §4.1 (the table, nine
/// hardness rungs × thirteen ρ rows, each rung normalised by its own
/// ρ < 0.05 core, zero point +0.00199) and §4.4 (the two cubics and the
/// `(m, n)` they reproduce) —
/// `b6-analysis.md` in the R29 materials ledger, outside the tree.
///
/// **The tolerances are the report's own numbers, not a bar tuned to pass.**
/// The deg-3 law scores pooled rms 0.0102 against this table (B6 §4.4's
/// "pooled 0.01020"), and its single largest cell deviation over the 99
/// cells with ρ < 1 is 0.0297 — at (ρ = 0.792, h = 0.125), which is the
/// worst rung in the report's own per-rung residual list. So the pooled bar
/// is 0.012 and the per-cell bar 0.035; anything that moves either
/// coefficient moves the pooled figure well past its bar.
///
/// MUTATION-LINED: flipping the sign of any cubic coefficient, or swapping
/// `m` and `n`, blows the pooled rms by more than an order of magnitude.
#[test]
fn brush_kernel_reproduces_the_measured_nine_rungs() {
    const HS: [f32; 9] = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0];
    // B6 §4.1, verbatim. The last two rows of the printed table (ρ = 1.002
    // and 1.042) are the ZERO check and are asserted separately below.
    const TABLE: [(f32, [f32; 9]); 11] = [
        (0.092, [0.9350, 0.9643, 0.9814, 0.9908, 0.9959, 0.9975, 0.9972, 0.9977, 0.9973]),
        (0.193, [0.7326, 0.8494, 0.9235, 0.9657, 0.9869, 0.9948, 0.9975, 0.9981, 0.9979]),
        (0.292, [0.4790, 0.6831, 0.8321, 0.9232, 0.9710, 0.9904, 0.9980, 0.9991, 0.9989]),
        (0.393, [0.2646, 0.5000, 0.7138, 0.8630, 0.9467, 0.9840, 0.9964, 0.9992, 0.9995]),
        (0.492, [0.1302, 0.3359, 0.5775, 0.7794, 0.9073, 0.9697, 0.9935, 0.9989, 0.9999]),
        (0.593, [0.0645, 0.2084, 0.4298, 0.6609, 0.8353, 0.9360, 0.9806, 0.9948, 0.9975]),
        (0.693, [0.0339, 0.1244, 0.2875, 0.5030, 0.7102, 0.8603, 0.9461, 0.9842, 0.9966]),
        (0.792, [0.0151, 0.0685, 0.1671, 0.3153, 0.4978, 0.6772, 0.8207, 0.9159, 0.9679]),
        (0.893, [0.0023, 0.0232, 0.0672, 0.1366, 0.2316, 0.3494, 0.4832, 0.6214, 0.7505]),
        (0.943, [0.0002, 0.0063, 0.0230, 0.0522, 0.0956, 0.1530, 0.2249, 0.3109, 0.4094]),
        (0.972, [-0.0007, 0.0007, 0.0046, 0.0126, 0.0246, 0.0424, 0.0656, 0.0954, 0.1318]),
    ];
    let (mut sq, mut cells, mut worst) = (0.0f64, 0usize, 0.0f32);
    for (rho, row) in TABLE {
        for (h, want) in HS.into_iter().zip(row) {
            let got = brush_kernel(rho, h);
            let d = got - want;
            sq += f64::from(d) * f64::from(d);
            cells += 1;
            worst = worst.max(d.abs());
            assert!(
                d.abs() <= 0.035,
                "k({rho}, {h}) = {got}, measured {want} (B6 §4.1); the law's own worst \
                 cell on this table is 0.0297"
            );
        }
    }
    let rms = (sq / cells as f64).sqrt();
    assert_eq!(cells, 99, "the whole ρ < 1 table, not a corner of it");
    assert!(rms <= 0.012, "pooled rms {rms} against B6 §4.4's own 0.01020");
    assert!(worst <= 0.035, "worst cell {worst}");

    // The two structural facts, which are measurements and not conventions:
    // the core is exactly 1 and the support ends exactly at ρ = 1 (B6 §4.1
    // reads |α| ≤ 5e-4 at every ρ ≥ 1.002 on every rung).
    for h in HS {
        assert_eq!(brush_kernel(0.0, h), 1.0, "k(0) = 1 at h = {h}");
        for rho in [1.0f32, 1.002, 1.042, 4.0] {
            assert_eq!(brush_kernel(rho, h), 0.0, "k({rho}) = 0 at h = {h}");
        }
    }
    // Monotone in h at EVERY ρ: B6 counted 0 inversions against batch-10's
    // 3 failures of 80, and it is the property that makes「harder = more
    // covered」true rather than approximately true.
    for (rho, _) in TABLE {
        let ks: Vec<f32> = HS.into_iter().map(|h| brush_kernel(rho, h)).collect();
        for w in ks.windows(2) {
            assert!(w[1] >= w[0] - 1e-6, "k is not monotone in h at ρ = {rho}: {ks:?}");
        }
    }
    // And the exponents themselves, against B6 §4.4's "Reproduced values"
    // (3 dp, so the bar is one unit in the last place plus rounding).
    const MN: [(f32, f32); 9] = [
        (1.832, 6.431),
        (1.664, 2.864),
        (1.907, 1.778),
        (2.593, 1.434),
        (3.932, 1.402),
        (6.249, 1.549),
        (9.790, 1.803),
        (14.210, 2.061),
        (17.964, 2.157),
    ];
    for (h, (m_w, n_w)) in HS.into_iter().zip(MN) {
        let (m, n) = brush_kernel_exponents(h);
        assert!((m - m_w).abs() <= 0.001, "m({h}) = {m}, B6 §4.4 prints {m_w}");
        assert!((n - n_w).abs() <= 0.001, "n({h}) = {n}, B6 §4.4 prints {n_w}");
    }
    // `h` outside Lightroom's own 0..1 is CLAMPED, not extrapolated: the
    // cubics are fits over exactly that interval and at h = 2 they return
    // m = 5e-4, which would paint the frame.
    assert_eq!(brush_kernel_exponents(-3.0), brush_kernel_exponents(0.0));
    assert_eq!(brush_kernel_exponents(9.0), brush_kernel_exponents(1.0));
    // NaN clamps to NEITHER end (`f32::clamp` propagates it), and a NaN
    // exponent turns the whole raster into black without a word.
    assert_eq!(brush_kernel_exponents(f32::NAN), brush_kernel_exponents(0.0));
}

/// The flow law against the MEASURED deposit — R29 Batch-6 §5.3.
///
/// `D(f) = κf/(1−f+κf)`, κ = 0.1284 (§5.4, four cells, 0.12836 ± 0.00288).
/// The four `mean` column values below are the deposits fitted per frame
/// under screen accumulation over the 5 × 2 × 2 drag grid; the law's
/// residual against them is +0.00099 / +0.00168 / +0.00193 / −0.00137, so
/// the bar is 0.0025.
///
/// MUTATION-LINED: the linear rival `D = 0.31252·f` — the best straight
/// line through these very points — misses them by rms 0.0382, which is the
/// 25× the last assert insists on (B6 quotes 12.7× over all sixteen cells).
#[test]
fn brush_flow_law_matches_the_measured_deposit() {
    const LADDER: [(f32, f32); 4] =
        [(0.10, 0.01307), (0.25, 0.03935), (0.50, 0.11182), (0.75, 0.27937)];
    let (mut odds_sq, mut lin_sq) = (0.0f64, 0.0f64);
    for (f, measured) in LADDER {
        let d = brush_flow_deposit(f);
        assert!(
            (d - measured).abs() <= 0.0025,
            "D({f}) = {d}, measured {measured} (B6 §5.3, max residual 0.00193)"
        );
        odds_sq += f64::from(d - measured).powi(2);
        lin_sq += f64::from(0.31252 * f - measured).powi(2);
    }
    // `D(1) = 1` EXACTLY and with no free parameter — κ/(1−1+κ) — which is
    // also exact in f32, so a flow-1 dab deposits its full density with no
    // epsilon (B6 §5.2 pins it at ≤ 0.0037 cost against a free fit).
    assert_eq!(brush_flow_deposit(1.0), 1.0, "D(1) must be exactly 1");
    assert_eq!(brush_flow_deposit(0.0), 0.0, "a flow-0 dab deposits nothing");
    // Off-domain flows clamp rather than run the odds law negative.
    assert_eq!(brush_flow_deposit(-1.0), 0.0);
    assert_eq!(brush_flow_deposit(7.0), 1.0);
    let (odds, lin) = ((odds_sq / 4.0).sqrt(), (lin_sq / 4.0).sqrt());
    assert!(
        lin > odds * 10.0,
        "the linear rival must lose decisively: odds {odds}, linear {lin}"
    );
}

/// Dabs accumulate by SCREEN — not by sum, not by max.
///
/// R29 Batch-6 §5.2 re-adjudicated this out-of-sample on 20 fresh drags:
/// mean field rms 0.01583 for screen against 0.02880 for sum-clamp (1.8×)
/// and 0.05343 for max (3.4×). Two overlapping dabs is the smallest fixture
/// that separates the three, and it separates them by far more than the
/// 8-bit raster quantum.
///
/// The flow is 0.75 on purpose: at low flow all three laws agree to within
/// a couple of quantisation steps, which is exactly how a sum-clamp
/// implementation could pass a weaker test.
#[test]
fn brush_dabs_accumulate_by_screen_not_sum_or_max() {
    let (fw, fh) = (480u32, 320u32);
    // Two dabs 0.2 apart, radius 0.25, so the frame centre sits at ρ = 0.4
    // from BOTH — an exact texel in x (240) and in y (160), so no bilinear
    // blend stands between the assertion and the accumulation.
    let g = probe_brush(&[(1.0, 0.25, 0.75, 0.5, "d 0.4 0.5\nd 0.6 0.5")]);
    let raster = brush_raster(&g, fw, fh).expect("two dabs");
    let got = mask_weight(&g, 0.5, 0.5, Some(&raster));
    let a = brush_flow_deposit(0.75) * brush_kernel(0.4, 0.5);
    let screen = 1.0 - (1.0 - a) * (1.0 - a);
    let sum = (a + a).min(1.0);
    let max = a;
    assert!(
        (got - screen).abs() <= 0.006,
        "α = {got}, screen says {screen} (8-bit raster, so the bar is ~1.5 quanta)"
    );
    assert!((got - sum).abs() >= 0.05, "sum-clamp would say {sum}");
    assert!((got - max).abs() >= 0.15, "max would say {max}");
    // Screen is also what makes a single dab reach exactly its deposit —
    // the premise the two-dab comparison stands on.
    let one = probe_brush(&[(1.0, 0.25, 0.75, 0.5, "d 0.5 0.5")]);
    let r1 = brush_raster(&one, fw, fh).expect("one dab");
    let solo = mask_weight(&one, 0.4, 0.5, Some(&r1));
    assert!((solo - a).abs() <= 0.006, "one dab deposits {a}, got {solo}");
}

/// The RASTER is the closed form, texel for texel — the join between the
/// measured model and the pixels.
///
/// The frame here is small enough that the raster is built 1:1, so this
/// reads the stamped texels DIRECTLY and the only error left is the 8-bit
/// quantum (1/255 = 0.0039, so ≤ 1/510 after rounding). Direct rather than
/// through `mask_weight`: since R29 C2 a texel's own normalised centre is
/// `(i + MASK_SAMPLE_CENTRE)/rw`, and going through the lookup to assert a
/// property OF the raster would only put a round trip between the claim and
/// the evidence. The lookup's half of the convention is pinned by
/// `bitmap_mask_sampling_matches_the_producers_convention` and
/// `every_mask_family_samples_at_pixel_centres`.
///
/// MUTATION-LINED: dropping the `value` (density) factor, or scaling by the
/// GROUP's `MaskValue` instead of the stroke's, moves every sample by 30 %.
#[test]
fn brush_raster_stamps_the_closed_form() {
    let (fw, fh) = (480u32, 320u32);
    // Density 0.7, flow 1 (so D = 1 exactly), one dab at the frame centre:
    // α(ρ) must be 0.7·k(ρ; h) with nothing else in the way.
    let g = probe_brush(&[(0.7, 0.25, 1.0, 0.5, "d 0.5 0.5")]);
    let raster = brush_raster(&g, fw, fh).expect("one dab");
    assert_eq!(raster.dimensions(), (fw, fh), "small frame = 1:1 raster");
    let texel = |i: u32, j: u32| raster.get_pixel(i, j)[0] as f32 / 255.0;
    // The dab's centre is texel coordinate 0.5·480 − 0.5 = 239.5 in x and
    // 0.5·320 − 0.5 = 159.5 in y — BETWEEN texels, because the frame has an
    // even number of them — and its half-extent is 120 texels on both axes.
    // Reading row 160 (half a texel below the centre), texel 240+d sits at
    //     ρ = √((d + 0.5)² + 0.5²) / 120.
    for d in [0i32, 12, 30, 60, 90, 114, 118] {
        let i = 240 + d;
        let rho = ((d as f32 + 0.5).powi(2) + 0.25).sqrt() / 120.0;
        let got = texel(i as u32, 160);
        let want = 0.7 * brush_kernel(rho, 0.5);
        assert!(
            (got - want).abs() <= 0.005,
            "texel ({i}, 160) (ρ = {rho}) reads {got}, the closed form says {want}"
        );
    }
    // …and stops. ρ = 1 is the outer support: on row 160 the support ends at
    // |i − 239.5| = √(120² − 0.5²) = 119.99896, so texel 359 is the last lit
    // one and 360 is blank — not faint.
    assert!(texel(359, 160) > 0.0, "texel 359 is inside the support");
    assert_eq!(texel(360, 160), 0.0, "support ends EXACTLY at ρ = 1 (B6 §4.1)");
    // A dab is a circle in PIXELS, not in normalised coordinates: at this
    // 3:2 frame the mask must reach 0.25 of the WIDTH on both axes, i.e.
    // 0.25·(480/320) = 0.375 of the HEIGHT. Both half-extents are therefore
    // 120 TEXELS, so ρ depends only on (Δi² + Δj²) and a pair of texels with
    // swapped offsets from (239.5, 159.5) must read EXACTLY equal.
    let horizontal = texel(347, 159); // Δ = (+107.5, −0.5)
    let vertical = texel(239, 267); //   Δ = (−0.5, +107.5)
    assert_eq!(horizontal, vertical, "the dab is not round in pixels");
    assert!(horizontal > 0.0, "premise: both probes are inside the dab");
}

/// A brush group with nothing drawable in it is INERT — including when the
/// group's own `crs:MaskInverted` is set, which is the one way a mask with
/// no coverage can become a whole-frame adjustment.
///
/// The three ways a group can carry strokes and paint nothing, all of them
/// states Lightroom also paints nothing for: no `d` token at all, a zero
/// radius, and flow 0.
#[test]
fn a_brush_group_with_no_drawable_dab_is_inert() {
    let (fw, fh) = (240u32, 160u32);
    for (what, stream, radius, flow) in [
        ("state tokens only", "r 0.2\nf 1\nh 0.5", 0.2f32, 1.0f32),
        ("zero radius", "d 0.5 0.5", 0.0, 1.0),
        ("zero flow", "d 0.5 0.5", 0.2, 0.0),
        ("garbage", "wobble\nd\nd 0.5", 0.2, 1.0),
    ] {
        let g = probe_brush(&[(1.0, radius, flow, 0.5, stream)]);
        assert!(brush_raster(&g, fw, fh).is_none(), "{what} must not rasterise");
        assert_eq!(mask_weight(&g, 0.5, 0.5, None), 0.0, "{what} must draw nothing");
        // The hazard, said in a test: `1 − 0` on an inverted group would
        // apply the correction to the WHOLE frame at full strength.
        let MaskGeometry::Brush { name, blend_mode, value, strokes, .. } = g else {
            unreachable!()
        };
        let flipped =
            MaskGeometry::Brush { name, blend_mode, value, inverted: true, strokes };
        assert_eq!(
            mask_weight(&flipped, 0.5, 0.5, None),
            0.0,
            "{what}: an inverted group with no alpha must not cover the frame"
        );
    }
    // A group with no strokes at all takes the same path.
    assert!(brush_raster(&probe_brush(&[]), fw, fh).is_none());
    // And a non-brush geometry answers `None` here rather than being asked
    // to explain itself at the call site.
    assert!(brush_raster(&MaskGeometry::Bitmap { path: "x.png".into() }, fw, fh).is_none());
}

/// The GROUP's own `crs:MaskInverted` IS rendered — `1 − α` — and it is the
/// only place it can be, because the importer deliberately does not lift it
/// into `LocalAdjustment::inverted` (xmp.rs: one bit, one home).
///
/// Measured `true` on 1 of 39 real groups (F2 anatomy), which is exactly
/// the population size that makes this worth a test rather than a comment.
#[test]
fn a_brush_groups_own_inversion_is_rendered() {
    let (fw, fh) = (480u32, 320u32);
    let plain = probe_brush(&[(1.0, 0.25, 1.0, 1.0, "d 0.5 0.5")]);
    let MaskGeometry::Brush { name, blend_mode, value, strokes, .. } = plain.clone() else {
        unreachable!()
    };
    let inverted = MaskGeometry::Brush { name, blend_mode, value, inverted: true, strokes };
    let raster = brush_raster(&plain, fw, fh).expect("one dab");
    // The SAME raster serves both: inversion is a reading of the alpha, not
    // a different alpha (which is also why it costs no second rasterise).
    assert_eq!(brush_raster(&inverted, fw, fh).map(|r| r.dimensions()), Some((fw, fh)));
    for (nx, ny) in [(0.5f32, 0.5f32), (0.55, 0.5), (0.02, 0.02)] {
        let w = mask_weight(&plain, nx, ny, Some(&raster));
        let f = mask_weight(&inverted, nx, ny, Some(&raster));
        assert!((w + f - 1.0).abs() <= 1e-6, "({nx}, {ny}): {w} + {f} != 1");
    }
}

/// The raster's SIZE policy, asserted at a cost the assertion does not have
/// to pay: exercising the work budget by really rasterising costs, by
/// construction, exactly the budget.
///
/// Three regimes, one function (`brush_raster_dims`): a small frame is 1:1,
/// a 61 MP frame is capped at the 2048 long edge, and a group heavy enough
/// to blow `BRUSH_RASTER_MAX_WORK` is shrunk until it fits — with
/// `BRUSH_RASTER_MIN_EDGE` overriding the budget rather than the mask being
/// destroyed.
#[test]
fn the_brush_raster_size_policy_is_bounded_three_ways() {
    // 1:1 while both bounds are slack — which is what makes a GUI preview's
    // lookup an exact texel hit rather than a bilinear blend.
    assert_eq!(brush_raster_dims(1_000.0, 480, 320), (480, 320));
    // The A7R IV frame, capped on the long edge and only there.
    assert_eq!(brush_raster_dims(1_000.0, 9504, 6336), (2048, 1365));
    // Heavy: 400 dabs each covering a 4000 × 3000 frame is 4.8e9 texels of
    // work at full size, so the budget takes the scale to sqrt(24e6/4.8e9).
    let heavy = 400.0 * 4000.0 * 3000.0;
    let (w, h) = brush_raster_dims(heavy, 4000, 3000);
    assert!(w < 2048, "the work budget must bite before the edge cap: {w}×{h}");
    assert!(w >= BRUSH_RASTER_MIN_EDGE, "and must not shrink past the floor: {w}");
    assert!(
        (f64::from(w) * f64::from(h) * 400.0) <= BRUSH_RASTER_MAX_WORK * 1.05,
        "the chosen size must actually meet the budget: {w}×{h}"
    );
    // The floor OVERRIDES the budget: an absurd cost cannot take the raster
    // to a single texel, because a 1 px mask is not a cheaper mask, it is a
    // wrong one. `BRUSH_MAX_DABS` is what bounds the overrun instead.
    let (w, h) = brush_raster_dims(f64::MAX, 4000, 3000);
    assert_eq!(w.max(h), BRUSH_RASTER_MIN_EDGE, "floor wins: {w}×{h}");
}
