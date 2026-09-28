
/// [`source_pixels`] is the ONE raw-vs-baked dispatch, so both arms and the
/// cap contract are pinned here.
///
/// The RAW arm cannot be exercised end-to-end without a real sensor file
/// (the repo carries no RAW fixture), but the DISPATCH can: a .ARW must
/// reach the develop engine — proven by the failure it produces being the
/// raw decoder's (`decoder_for`'s "does not read as any RAW format"),
/// never `load_image`'s "is a camera RAW, and this step reads finished
/// images" refusal. That refusal appearing here would mean the gate had
/// sent a RAW down the baked arm, which is exactly the v0.22 mask-refine
/// bug. The needle is the refusal's own clause, not the bare "is a camera
/// RAW": the decoder's message says "if it really is a camera RAW" too.
#[test]
fn the_source_dispatch_sends_each_kind_down_its_own_arm() {
    let dir = std::env::temp_dir().join(format!("autoshade-source-px-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let raw = dir.join("fake.arw");
    std::fs::write(&raw, b"not really a raw").unwrap();
    let e = format!("{:#}", source_pixels(&raw, None).unwrap_err());
    assert!(
        !e.contains("is a camera RAW, and this step reads finished images"),
        "a RAW must be DEVELOPED, not sent to the baked decoder: {e}"
    );
    assert!(
        e.contains("does not read as any RAW format"),
        "the failure is the RAW decoder's own: {e}"
    );

    // Baked arm: full resolution when uncapped...
    let big = dir.join("big.png");
    image::RgbImage::from_fn(400, 200, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, 7]))
        .save(&big)
        .unwrap();
    assert_eq!(source_pixels(&big, None).unwrap().dimensions(), (400, 200));
    // ...bounded by the cap's LONG edge, aspect kept...
    assert_eq!(source_pixels(&big, Some(100)).unwrap().dimensions(), (100, 50));
    // ...and NEVER upsampled: `thumbnail` alone inflates a small source,
    // which would hand a heal/denoise/refine consumer invented pixels (and
    // save them as the delivered master).
    let small = dir.join("small.png");
    image::RgbImage::from_pixel(64, 48, image::Rgb([3, 4, 5])).save(&small).unwrap();
    assert_eq!(source_pixels(&small, Some(2048)).unwrap().dimensions(), (64, 48));
    let _ = std::fs::remove_dir_all(&dir);
}

/// R27 P3 — the baked develop takes a working-resolution cap, and obeys
/// the RAW arm's two rules for it: bound the LONG edge, and only ever go
/// DOWN.
///
/// Before this the baked arm had no cap while the RAW arm had one (the
/// format map's B2), so a 60 MP TIFF developed at full size wherever it
/// was developed at all. The downscale-only half matters as much as the
/// bound: plain `thumbnail` UPSCALES a source smaller than the cap, which
/// would invent pixels and then hand them on as a developed master.
///
/// MUTATION THIS CATCHES: use `resize` instead of `thumbnail`'s
/// downscale-only guard and the third assertion inflates the 64×48 source
/// to 2048×1536; cap AFTER the develop instead of before and the first
/// assertion still passes while the memory saving — the entire point —
/// silently disappears, which is why the fourth assertion pins that the
/// tone stage saw the SMALL frame.
#[test]
fn the_baked_develop_caps_its_working_resolution_downward_only() {
    let big = DynamicImage::ImageRgb8(image::RgbImage::from_fn(400, 200, |x, y| {
        image::Rgb([(x % 251) as u8, (y % 241) as u8, 7])
    }));
    let r = EditRecipe::default();

    // Uncapped = the source's own resolution (what the export path asks).
    assert_eq!(
        render_baked_to_image(&big, &r, None, None, &crate::diag::pixels()).unwrap().dimensions(),
        (400, 200)
    );
    // Capped on the LONG edge, aspect kept.
    assert_eq!(
        render_baked_to_image(&big, &r, None, Some(100), &crate::diag::pixels()).unwrap().dimensions(),
        (100, 50)
    );
    // Never upsampled.
    let small = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(64, 48, image::Rgb([3, 4, 5])));
    assert_eq!(
        render_baked_to_image(&small, &r, None, Some(2048), &crate::diag::pixels()).unwrap().dimensions(),
        (64, 48)
    );

    // The cap runs BEFORE the develop, not after: a crop is expressed in
    // normalised coordinates on the developed frame, so if the shrink
    // happened last the crop would be taken from the 400-wide frame and
    // then shrunk, landing on 100×50 either way. Ask instead for a frame
    // whose SIZE reveals the order — a half-width crop of a capped
    // develop is 50 px; of an uncapped one shrunk afterwards it would be
    // 200 px before the shrink and the function returns the crop, not the
    // cap, so the two disagree.
    let cropped = EditRecipe {
        crop: Some(crate::recipe::Crop { left: 0.0, top: 0.0, right: 0.5, bottom: 1.0 }),
        ..Default::default()
    };
    assert_eq!(
        render_baked_to_image(&big, &cropped, None, Some(100), &crate::diag::pixels()).unwrap().dimensions(),
        (50, 50),
        "the crop must be taken from the CAPPED frame — i.e. the cap ran first"
    );
}

/// Patrol (the `find_raws_accepts_every_raw_format_the_app_can_decode`
/// pattern: ONE predicate, app-wide): every line in the tree that names the
/// baked decoder must say, ON THE LINE, why it is allowed to. A new consumer
/// of *source* pixels must go through [`source_pixels`] instead — the whole
/// point of having one dispatch.
///
/// Per CALL SITE, not per file (R22 M2). The old form asserted a sorted FILE
/// allow-list, so any file already on it could grow a new hand-rolled decode
/// of source pixels and stay green — including `bin/gui/workers.rs`, the
/// exact site of the v0.22 "AI mask refine failed" accident this patrol was
/// written for. Two markers, either on the line or on the line above it:
///
/// * `// baked-by-construction: <why this path can never be a camera RAW>`
///   — a consumer's call.
/// * `// not-a-consumer-call: <why the line is not a consumer at all>` — the
///   gate's own declaration / dispatch / unit tests, and this patrol's own
///   extractor literal. It exists so those lines stay HONEST instead of
///   claiming a baked path they do not have.
///
/// Lexical on purpose: the drift this catches is someone typing
/// `load_image` in a new worker, which no type can prevent (both arms
/// return `DynamicImage`). Known and accepted limit, unchanged from the
/// file-granular form: a `//`-prefixed line is skipped (that is where the
/// doc references live), and a mention inside a string literal is scanned
/// like a call — the marker is then the honest answer.
#[test]
fn every_baked_decode_line_says_why_it_is_allowed() {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).expect("source dir listable") {
            let p = e.expect("dir entry").path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&src, &mut files);
    files.sort();
    assert!(files.len() >= 20, "only {} source files walked — the patrol is broken", files.len());

    const MARKERS: [&str; 2] = ["baked-by-construction:", "not-a-consumer-call:"];
    let mut scanned = 0usize;
    let mut unmarked: Vec<String> = Vec::new();
    for p in &files {
        let text = std::fs::read_to_string(p).expect("source readable");
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            // Comment lines carry the DOC references, which are exactly what
            // this patrol must not count.
            // not-a-consumer-call: the patrol's own extractor literal.
            if line.trim_start().starts_with("//") || !line.contains("load_image") {
                continue;
            }
            scanned += 1;
            let above = i.checked_sub(1).map(|j| lines[j]).unwrap_or("");
            if MARKERS.iter().any(|m| line.contains(m) || above.contains(m)) {
                continue;
            }
            unmarked.push(format!(
                "{}:{}: {}",
                p.strip_prefix(&src).unwrap_or(p).to_string_lossy().replace('\\', "/"),
                i + 1,
                line.trim()
            ));
        }
    }
    assert!(scanned >= 20, "only {scanned} lines scanned — the extractor is broken");
    assert!(
        unmarked.is_empty(),
        "these lines name the baked decoder with no reason on them — route the source through \
         render::source_pixels, or mark the line `// baked-by-construction: <why>` (a path that \
         cannot be a RAW) / `// not-a-consumer-call: <why>`:\n{}",
        unmarked.join("\n")
    );
}

/// L01-6: two active vignette stages compose into ONE clamped pass — the
/// per-pass clamp made mathematically inverse corrections irreversible on
/// bright pixels.
#[test]
fn opposing_vignette_stages_compose_into_one_clamped_pass() {
    let knots = vec![2.0f32; 4];
    let p_lut = profile_vignette_lut(&knots, 100.0);
    let m_lut = manual_vignette_lut(-50.0, 50.0);
    let r = EditRecipe {
        lens_vignette: -50.0,
        lens_profile: crate::recipe::LensProfile {
            vignette: knots,
            vignette_on: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let lut = vignette_gain_lut(&r).expect("both stages active");
    let last = *lut.last().unwrap();
    assert!(
        (last - p_lut.last().unwrap() * m_lut.last().unwrap()).abs() < 1e-6,
        "the composed LUT is the product of the stage gains"
    );
    assert!((last - 1.0).abs() < 1e-3, "2.0 × 0.5 cancels at the corner: {last}");

    // A bright corner pixel survives the composed pass untouched, where
    // the clamped two-pass order permanently darkened it. 9×9: the
    // radial geometry floors rmax at one PIXEL, so a frame this small
    // is needed for the corner to actually reach rn = 1.0.
    let mut composed = vec![[0.9f32; 3]; 81];
    apply_radial_gain(&mut composed, 9, 9, &lut);
    let mut two_pass = vec![[0.9f32; 3]; 81];
    apply_radial_gain(&mut two_pass, 9, 9, &p_lut);
    apply_radial_gain(&mut two_pass, 9, 9, &m_lut);
    assert!((composed[0][0] - 0.9).abs() < 1e-3, "composed: {}", composed[0][0]);
    assert!(two_pass[0][0] < 0.85, "the clamp loses the highlight: {}", two_pass[0][0]);
}

use super::*;
use crate::recipe::{EditRecipe, LocalAdjustment};

#[test]
fn base_curve_composes_under_user_tone_and_is_not_skipped() {
    // Mid-grey through a lifting base curve must brighten even with a
    // fully-neutral user recipe — the neutral short-circuit must treat the
    // base look as tone work — and user exposure must act ON TOP of it.
    let base =
        DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, image::Rgb([100, 100, 100])));
    let mut lifted = EditRecipe {
        base_curve: vec![[0.0, 0.0], [0.4, 0.62], [1.0, 1.0]],
        ..Default::default()
    };
    let neutral_px = develop_preview(&base, &EditRecipe::default()).to_rgb8()[(0, 0)][0];
    assert_eq!(neutral_px, 100, "a truly neutral recipe is the identity");
    let lifted_px = develop_preview(&base, &lifted).to_rgb8()[(0, 0)][0];
    assert!(lifted_px > 130, "base curve must lift mid-grey: {lifted_px}");
    lifted.exposure_ev = 1.0;
    let more_px = develop_preview(&base, &lifted).to_rgb8()[(0, 0)][0];
    assert!(more_px > lifted_px, "exposure composes on top of the base look: {more_px}");
    // Endpoints stay pinned through the composition: black in → black out.
    let black = DynamicImage::ImageRgb8(RgbImage::from_pixel(4, 4, image::Rgb([0, 0, 0])));
    lifted.exposure_ev = 0.0;
    assert_eq!(develop_preview(&black, &lifted).to_rgb8()[(0, 0)][0], 0);
}

/// v1.5.0 F7, END TO END: a creative profile's baked half reaches the
/// PIXELS, in the direction the profile states.
///
/// This is the claim the whole batch rests on and the one the unit tests
/// underneath it cannot make. `render::build_tone_lut`,
/// `render::apply_rgb_curves` and `EditRecipe::with_baked` each read the
/// Look, and every one of them could have been written correctly while
/// nothing called them — a develop panel that shows a profile's name over
/// an unchanged photograph is exactly the 「a slider that moves a number
/// and no pixel」 failure this file's rules call the worst kind here.
///
/// The curve is `Adobe Color`'s own, measured off the sidecars: 161 of the
/// 175 in the reference library carry a Look and 152 of those are this one,
/// so this S is what almost every Lightroom photograph is developed
/// through. It darkens the low end (22→16, 40→35) and lifts the high end
/// (224→230, 240→246) about a pinned midpoint (127→127).
///
/// MUTATION: drop any of the four `baked_look`/`with_baked` reads in this
/// file, or let `amount` scale nothing.
#[test]
fn a_baked_creative_profile_reaches_the_pixels_in_its_own_direction() {
    use crate::recipe::{CreativeLook, CurvePoint};
    let knot = |input, output| CurvePoint { input, output };
    let adobe_color = || CreativeLook {
        name: "Adobe Color".to_string(),
        amount: 1.0,
        tone_curve: vec![
            knot(0, 0),
            knot(22, 16),
            knot(40, 35),
            knot(127, 127),
            knot(224, 230),
            knot(240, 246),
            knot(255, 255),
        ],
        ..Default::default()
    };
    let patch = |v: u8| DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, image::Rgb([v; 3])));
    let shot = |img: &DynamicImage, r: &EditRecipe| develop_preview(img, r).to_rgb8()[(0, 0)][0];

    let bare = EditRecipe::default();
    let profiled = EditRecipe { look: Some(adobe_color()), ..Default::default() };

    // The two ends move the way the curve says, and the pinned middle does
    // not move at all — which is what makes this the curve's shape rather
    // than a brightness offset that happens to be signed correctly.
    let (dark_bare, dark_profiled) = (shot(&patch(40), &bare), shot(&patch(40), &profiled));
    let (lit_bare, lit_profiled) = (shot(&patch(224), &bare), shot(&patch(224), &profiled));
    let (mid_bare, mid_profiled) = (shot(&patch(127), &bare), shot(&patch(127), &profiled));
    assert!(dark_profiled < dark_bare, "40→35 must darken: {dark_bare} → {dark_profiled}");
    assert!(lit_profiled > lit_bare, "224→230 must lift: {lit_bare} → {lit_profiled}");
    assert!(
        mid_profiled.abs_diff(mid_bare) <= 1,
        "127→127 is pinned: {mid_bare} → {mid_profiled}"
    );

    // `crs:Amount` scales the baked half, so half of it lands strictly
    // between doing nothing and doing all of it.
    let half = EditRecipe {
        look: Some(CreativeLook { amount: 0.5, ..adobe_color() }),
        ..Default::default()
    };
    let dark_half = shot(&patch(40), &half);
    assert!(
        dark_profiled < dark_half && dark_half < dark_bare,
        "amount 0.5 sits between: {dark_bare} / {dark_half} / {dark_profiled}"
    );

    // The baked SLIDERS add to the photographer's own rather than replacing
    // them (`EditRecipe::with_baked`): the same −40 shadows pulls further
    // down when the profile bakes −40 too.
    let img = patch(90);
    let own = EditRecipe { shadows: -40.0, ..Default::default() };
    let both = EditRecipe {
        shadows: -40.0,
        look: Some(CreativeLook { shadows: -40.0, ..adobe_color() }),
        ..Default::default()
    };
    assert!(
        shot(&img, &both) < shot(&img, &own),
        "a baked shadow move ADDS: {} vs {}",
        shot(&img, &both),
        shot(&img, &own)
    );

    // …and a monochrome profile develops to grey with the photographer's
    // own Black & White switch still off.
    let mono = EditRecipe {
        look: Some(CreativeLook {
            name: "Adobe Monochrome".to_string(),
            grayscale: true,
            ..adobe_color()
        }),
        ..Default::default()
    };
    assert!(!mono.convert_to_grayscale, "the photographer never touched the switch");
    let colourful =
        DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, image::Rgb([200, 60, 40])));
    let px = develop_preview(&colourful, &mono).to_rgb8()[(0, 0)];
    let (hi, lo) = (px[0].max(px[1]).max(px[2]), px[0].min(px[1]).min(px[2]));
    assert!(hi - lo <= 1, "a monochrome profile must develop grey: {px:?}");
    // The control: the very same frame through the colour profile is not.
    let colour_px = develop_preview(&colourful, &profiled).to_rgb8()[(0, 0)];
    assert!(
        colour_px[0] > colour_px[2] + 20,
        "…and the colour profile leaves it red: {colour_px:?}"
    );
}

/// v1.5.0 F9, end to end: an area the photographer removed in Lightroom
/// comes off the photograph here too.
///
/// This test exists because F7 taught the lesson at cost — two shipped
/// defects that each masked the other, invisible to every unit test,
/// because the only thing that could see them was a photograph going in
/// one end and coming out the other. Both retouch geometries go through
/// the public develop entry point here, for the same reason.
///
/// `feather: 0.0` on purpose: it makes the healed disc's edge crisp, so
/// "was the radius converted" is a question about which pixels changed
/// rather than about how far a ramp reached.
///
/// MUTATION: drop the `heal_planar` call; drop `to_short` from the radius
/// (the 7.5 px disc leaves the blob's rim behind); rasterise the brush at
/// the wrong frame size.
#[test]
fn a_lightroom_retouch_area_takes_the_object_off_the_photograph() {
    use crate::retouch::{RetouchArea, RetouchShape, SpotOrigin};
    // 160x120 flat grey with a black disc of radius 8 px at (40, 60).
    let (w, h) = (160u32, 120u32);
    let mut src = RgbImage::from_pixel(w, h, image::Rgb([140u8; 3]));
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f32 - 40.0, y as f32 - 60.0);
            if dx * dx + dy * dy <= 64.0 {
                src.put_pixel(x, y, image::Rgb([0, 0, 0]));
            }
        }
    }
    let img = DynamicImage::ImageRgb8(src);
    // The darkest pixel anywhere in the blob, and a far-field control.
    // Stated against the SAME rendition's own background rather than
    // against an absolute code value, so the assertion does not depend on
    // what the rest of the develop chain does to a flat grey.
    let darkest = |d: &DynamicImage| {
        let rgb = d.to_rgb8();
        let mut m = 255u8;
        for y in 51..70u32 {
            for x in 31..50u32 {
                let (dx, dy) = (x as f32 - 40.0, y as f32 - 60.0);
                if dx * dx + dy * dy <= 64.0 {
                    m = m.min(rgb[(x, y)][0]);
                }
            }
        }
        m
    };
    let far = |d: &DynamicImage| d.to_rgb8()[(130, 25)][0];

    // The control: an empty recipe leaves the object exactly where it is.
    let bare = develop_preview(&img, &EditRecipe::default());
    assert!(
        darkest(&bare) + 80 < far(&bare),
        "the blob must survive an empty recipe: {} vs {}",
        darkest(&bare),
        far(&bare)
    );

    // ELLIPSE. `size_x` is a half-extent in WIDTH units — 10 px of 160 —
    // which is 10 px of the 120 px short side once converted, and so
    // covers the 8 px blob. Without the conversion it is 7.5 px and the
    // rim survives.
    let ellipse = RetouchArea {
        origin: SpotOrigin::LightroomContentAware,
        feather: 0.0,
        donor: None,
        shape: RetouchShape::Ellipse {
            cx: 40.0 / 160.0,
            cy: 60.0 / 120.0,
            size_x: 10.0 / 160.0,
            size_y: 10.0 / 160.0,
        },
    };
    let healed = develop_preview(
        &img,
        &EditRecipe { retouch: vec![ellipse.clone()], ..Default::default() },
    );
    assert!(
        darkest(&healed) + 20 > far(&healed),
        "every pixel of the area must heal to its surroundings: {} vs {}",
        darkest(&healed),
        far(&healed)
    );

    // BRUSH — the other 37 of the library's 121 areas. One dab at the same
    // place, the same width-unit radius, through the mask side's own
    // rasteriser and the retouch side's own planner.
    let brush = RetouchArea {
        shape: RetouchShape::Brush(vec![crate::recipe::BrushStroke {
            value: 1.0,
            radius: 10.0 / 160.0,
            flow: 1.0,
            center_weight: 1.0,
            sync_id: String::new(),
            dabs: "d 0.250000 0.500000".to_string(),
        }]),
        ..ellipse.clone()
    };
    let brushed = develop_preview(
        &img,
        &EditRecipe { retouch: vec![brush], ..Default::default() },
    );
    assert!(
        darkest(&brushed) + 20 > far(&brushed),
        "a brush-shaped area heals too: {} vs {}",
        darkest(&brushed),
        far(&brushed)
    );
}

/// v1.5.0 F9: a retouch area turns with the frame, and comes home.
///
/// Lightroom states these coordinates in the UN-ROTATED SENSOR frame, like
/// a crop rectangle and like a brush dab, and this is not a corner case —
/// 7 of the 25 retouched photographs in the reference library are
/// `tiff:Orientation="8"`. An area left un-turned is repaired a quarter
/// turn from the object: a clean patch of sky rewritten and the power line
/// still there.
///
/// A round trip, plus the assertion that makes a round trip mean
/// something — that the first turn MOVED it. Without that, deleting the
/// whole arm passes this test twice over.
///
/// MUTATION: drop either arm of the `r.retouch` loop; turn the centre but
/// not the half-extents; hand the second turn the un-swapped frame.
#[test]
fn a_retouch_area_turns_with_the_frame_and_comes_home() {
    use crate::recipe::BrushStroke;
    use crate::retouch::{RetouchArea, RetouchShape, SpotOrigin};
    use rawler::Orientation;
    // 3:2, so a quarter turn really changes which edge "width" names and a
    // half-extent that rode through unscaled would come back 1.5x wrong.
    let area = |shape| RetouchArea {
        origin: SpotOrigin::LightroomContentAware,
        feather: 0.5,
        donor: Some([0.6, 0.4]),
        shape,
    };
    let mut r = EditRecipe {
        retouch: vec![
            area(RetouchShape::Ellipse { cx: 0.25, cy: 0.5, size_x: 0.05, size_y: 0.05 }),
            area(RetouchShape::Brush(vec![BrushStroke {
                value: 1.0,
                radius: 0.05,
                flow: 1.0,
                center_weight: 1.0,
                sync_id: String::new(),
                dabs: "d 0.250000 0.500000".to_string(),
            }])),
        ],
        ..Default::default()
    };
    let before = r.retouch.clone();

    orient_recipe_coords(&mut r, Orientation::Rotate90, CoordFrame::new(9504.0, 6336.0));
    assert_ne!(r.retouch, before, "a quarter turn must MOVE a retouch area");
    // Both arms moved, not just the one the loop happens to reach first.
    match (&before[0].shape, &r.retouch[0].shape) {
        (
            RetouchShape::Ellipse { cx: c0, size_x: s0, .. },
            RetouchShape::Ellipse { cx: c1, size_x: s1, .. },
        ) => {
            assert_ne!(c0, c1, "the ellipse's centre turns");
            assert_ne!(s0, s1, "…and its half-extent rescales with the new width");
        }
        ref other => panic!("two ellipses, got {other:?}"),
    }
    match (&before[1].shape, &r.retouch[1].shape) {
        (RetouchShape::Brush(b0), RetouchShape::Brush(b1)) => {
            assert_ne!(b0[0].dabs, b1[0].dabs, "the brush's dabs turn too");
        }
        ref other => panic!("two brushes, got {other:?}"),
    }
    assert_ne!(before[0].donor, r.retouch[0].donor, "and so does the donor point");

    // Home again, through the TURNED frame — the quarter turns are each
    // other's inverse, so the second one is handed 6336x9504.
    orient_recipe_coords(&mut r, Orientation::Rotate270, CoordFrame::new(6336.0, 9504.0));
    // A tolerance, not equality: the half-extent is multiplied by 1.5 and
    // then by its reciprocal in f32, which is exact for the dab grid the
    // writer rounds to and within a millionth here.
    for (b, a) in before.iter().zip(&r.retouch) {
        match (&b.shape, &a.shape) {
            (
                RetouchShape::Ellipse { cx: x0, cy: y0, size_x: s0, size_y: t0 },
                RetouchShape::Ellipse { cx: x1, cy: y1, size_x: s1, size_y: t1 },
            ) => {
                for (was, now, what) in
                    [(x0, x1, "cx"), (y0, y1, "cy"), (s0, s1, "size_x"), (t0, t1, "size_y")]
                {
                    assert!(
                        (was - now).abs() < 1e-6,
                        "{what} must come home: {was} → {now}"
                    );
                }
            }
            (RetouchShape::Brush(b0), RetouchShape::Brush(b1)) => {
                assert_eq!(b0[0].dabs, b1[0].dabs, "the dab stream comes home verbatim");
                assert!((b0[0].radius - b1[0].radius).abs() < 1e-6, "so does its radius");
            }
            ref other => panic!("same shapes both ways, got {other:?}"),
        }
        let (Some(d0), Some(d1)) = (b.donor, a.donor) else { panic!("both donors survive") };
        assert!(
            (d0[0] - d1[0]).abs() < 1e-6 && (d0[1] - d1[1]).abs() < 1e-6,
            "the donor comes home: {d0:?} → {d1:?}"
        );
    }
}

/// v1.5.0 F9: the two unit conversions `retouch_spots` owns, pinned as
/// numbers rather than inferred from a rendition.
///
/// Both are places where Lightroom and this engine mean different things
/// by the same word, and both are silent when wrong — a radius off by the
/// aspect ratio still heals SOMETHING, and a donor read as an offset still
/// copies SOME pixels.
///
/// MUTATION: normalise the radius by the long side or by width; read the
/// donor without subtracting the spot centre.
#[test]
fn a_retouch_areas_units_become_this_engines_at_exactly_one_place() {
    use crate::retouch::{RetouchArea, RetouchShape, SpotOrigin};
    let r = EditRecipe {
        retouch: vec![
            RetouchArea {
                origin: SpotOrigin::LightroomHeal,
                feather: 0.5,
                // ABSOLUTE, whatever `crs:OffsetY` sounds like.
                donor: Some([0.60, 0.40]),
                shape: RetouchShape::Ellipse {
                    cx: 0.25,
                    cy: 0.50,
                    size_x: 0.05,
                    size_y: 0.05,
                },
            },
            // A BRUSH area, which reaches the spots by the other road —
            // rasterised, then planned by connected components. The sweep
            // is what put it here: the planner takes everything except the
            // geometry from a `HealSpot` template, and with only an
            // ellipse in this test that template could be replaced by
            // `Default::default()` with nothing going red.
            RetouchArea {
                origin: SpotOrigin::LightroomGenerative,
                feather: 0.25,
                donor: None,
                shape: RetouchShape::Brush(vec![crate::recipe::BrushStroke {
                    value: 1.0,
                    radius: 0.05,
                    flow: 1.0,
                    center_weight: 1.0,
                    sync_id: String::new(),
                    dabs: "d 0.750000 0.500000".to_string(),
                }]),
            },
        ],
        ..Default::default()
    };
    // A 160x120 frame: width 160, short side 120, so a width-unit
    // half-extent of 0.05 (8 px) is 8/120 of the short side.
    let spots = super::retouch_spots(&r, 160, 120);
    assert_eq!(spots.len(), 2, "one spot from the ellipse, one from the stroke");
    // The brush's spot carries what the RASTER cannot say.
    assert_eq!(
        spots[1].origin,
        SpotOrigin::LightroomGenerative,
        "a planned spot keeps its area's provenance"
    );
    assert_eq!(spots[1].feather, 0.25, "…and its area's feather");
    assert!(spots[1].coverage.is_some(), "…and the stroke's exact shape");
    assert!(
        (spots[0].radius - 8.0 / 120.0).abs() < 1e-6,
        "0.05 of WIDTH is 8 px is {} of the short side, got {}",
        8.0 / 120.0,
        spots[0].radius
    );
    let source = spots[0].source.expect("a stated donor survives");
    assert!(
        (source[0] - 0.35).abs() < 1e-6 && (source[1] + 0.10).abs() < 1e-6,
        "an absolute donor becomes an offset from the spot centre: got {source:?}"
    );
    assert_eq!(spots[0].feather, 0.5, "the area's own feather rides through");
    assert_eq!(spots[0].origin, SpotOrigin::LightroomHeal, "so does its provenance");
}

/// v1.5.0 F8: the HDR switch is a GATE, and the SDR rendition really acts.
///
/// Both halves in one test, because each is worthless without the other.
/// A gate that is always shut renders nothing and passes "the seven do not
/// act while the mode is off"; a rendition with no gate passes "the seven
/// act" and re-tones every SDR photograph in the archive. So: the same
/// seven values, twice, and the ONLY difference is the switch.
///
/// MUTATION: drop the `!r.hdr_edit` early return in `SdrRendition::global`
/// (the gated arm stops matching the bare one); drop the `hdr::apply` call
/// at the end of the chain (the HDR arm stops differing).
#[test]
fn hdr_edit_mode_gates_the_sdr_rendition_and_the_rendition_renders() {
    // A vertical ramp: every code value present, so a tone move anywhere
    // in the range has somewhere to show.
    let (w, h) = (64u32, 256u32);
    let mut src = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = y as u8;
            src.put_pixel(x, y, image::Rgb([v, v, v]));
        }
    }
    let img = DynamicImage::ImageRgb8(src);
    let sdr = EditRecipe {
        sdr_brightness: 40.0,
        sdr_contrast: 30.0,
        sdr_highlights: -50.0,
        sdr_shadows: 25.0,
        sdr_whites: -20.0,
        sdr_clarity: 20.0,
        ..Default::default()
    };
    let bare = develop_preview(&img, &EditRecipe::default());
    let gated = develop_preview(&img, &sdr);
    assert_eq!(
        bare.to_rgb8().as_raw(),
        gated.to_rgb8().as_raw(),
        "seven SDR controls outside HDR edit mode must not move one pixel"
    );
    let on = develop_preview(&img, &EditRecipe { hdr_edit: true, ..sdr.clone() });
    assert_ne!(
        bare.to_rgb8().as_raw(),
        on.to_rgb8().as_raw(),
        "…and inside it they must move the photograph"
    );
    // The direction, so a rendition that merely perturbs the frame cannot
    // pass: +40 Brightness on a mid ramp value lifts it.
    let mid = |d: &DynamicImage| d.to_rgb8()[(32, 96)][0];
    assert!(
        mid(&on) > mid(&bare),
        "+40 SDR Brightness lifts the midtone: {} → {}",
        mid(&bare),
        mid(&on)
    );
    // …and Brightness ALONE lifts it. The arm above moves five sliders at
    // once, so it cannot see which one did the lifting: deleting
    // Brightness entirely left Contrast and Shadows holding the midtone up
    // and the assertion green (falsification case F8-M6). One slider, one
    // probe.
    let bright = develop_preview(
        &img,
        &EditRecipe { hdr_edit: true, sdr_brightness: 40.0, ..Default::default() },
    );
    assert!(
        mid(&bright) > mid(&bare),
        "SDR Brightness is this rendition's exposure and must act by itself: {} → {}",
        mid(&bare),
        mid(&bright)
    );
}

/// v1.5.0 F8: the HEADROOM alone renders, with every SDR slider at 0, and
/// it renders as a shoulder — the highlights come down, the shadows do not.
///
/// A headroom that reached the picture as an exposure cut, or as contrast,
/// would fail the second assertion; one that reached it as a highlights
/// LIFT would fail the first. Those four properties are the shoulder's
/// contract and they held across the 2026-09-19 change of mechanism, from a
/// negative Highlights push to the headroom's own curve — which is what a
/// contract is for.
///
/// MUTATION: flip the shoulder's sign; drop `hdr_max_ev` from
/// `hdr::shoulder`'s white point; drop the blend term (the two headroom
/// arms converge).
#[test]
fn the_hdr_headroom_reaches_the_picture_as_a_shoulder() {
    let (w, h) = (64u32, 256u32);
    let mut src = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = y as u8;
            src.put_pixel(x, y, image::Rgb([v, v, v]));
        }
    }
    let img = DynamicImage::ImageRgb8(src);
    let at = |d: &DynamicImage, y: u32| d.to_rgb8()[(32, y)][0] as i32;
    let bare = develop_preview(&img, &EditRecipe::default());
    let one = develop_preview(
        &img,
        &EditRecipe { hdr_edit: true, hdr_max_ev: 1.0, ..Default::default() },
    );
    assert!(
        at(&one, 230) < at(&bare, 230),
        "one stop of headroom pulls the highlights down: {} → {}",
        at(&bare, 230),
        at(&one, 230)
    );
    assert!(
        (at(&one, 20) - at(&bare, 20)).abs() <= 1,
        "…and leaves the shadows where they were: {} → {}",
        at(&bare, 20),
        at(&one, 20)
    );
    // Blend scales it, and in the stated direction: −100 is no shoulder at
    // all, +100 is twice one.
    let off = develop_preview(
        &img,
        &EditRecipe {
            hdr_edit: true,
            hdr_max_ev: 1.0,
            sdr_blend: -100.0,
            ..Default::default()
        },
    );
    assert_eq!(
        bare.to_rgb8().as_raw(),
        off.to_rgb8().as_raw(),
        "SDRBlend −100 means the headroom does not reach the rendition at all"
    );
    let twice = develop_preview(
        &img,
        &EditRecipe {
            hdr_edit: true,
            hdr_max_ev: 1.0,
            sdr_blend: 100.0,
            ..Default::default()
        },
    );
    assert!(
        at(&twice, 230) < at(&one, 230),
        "…and +100 reaches further than 0: {} vs {}",
        at(&one, 230),
        at(&twice, 230)
    );
}

/// v1.5.0 F8: the headroom keeps reaching at every headroom the sidecar can
/// state — it is not governed by a taste control's limiter.
///
/// The defect this pins, measured off the kit's `HDR-ON` on 2026-09-19:
/// the shoulder used to be a negative Highlights push, so it inherited
/// [`limit_tone_sliders`], whose rule is that a slider must SATURATE and
/// never annihilate a tonal band. Correct for a slider a photographer
/// drags; wrong for a rendering transform. The shoulder stopped growing
/// partway up the headroom range and reached 55 % of Lightroom's, and no
/// larger `crs:HDRMaxValue` could recover the rest.
///
/// So this asserts the DEPTH at the headroom the kit itself states, which
/// is the quantity that separated the two models. At `crs:HDRMaxValue` of
/// +2.30 the old model reached 0.104 below the diagonal at input 0.90 where
/// Lightroom reaches 0.189; the curve reaches 0.193. On this ramp that is
/// about 27 code values against about 50, so a floor of 40 sits clear of
/// both. Direction alone does not catch this — the capped model came down
/// too, just not far enough, which is how it passed its tests for a whole
/// batch.
///
/// Each further stop still reaches further, but by LESS, and that is the
/// curve's own form rather than a cap: extended Reinhard tends to
/// `L/(1+L)` as its white point grows, so headroom has diminishing returns
/// on an input that is itself bounded. This test asserted the opposite when
/// it was first written and the measurement corrected it.
///
/// MUTATION: clamp `stops` in `SdrRendition::global` to 1.0, or route the
/// shoulder back through the highlights slot — the depth falls under 40.
#[test]
fn the_headroom_keeps_reaching_past_where_a_slider_would_saturate() {
    let (w, h) = (64u32, 256u32);
    let mut src = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = y as u8;
            src.put_pixel(x, y, image::Rgb([v, v, v]));
        }
    }
    let img = DynamicImage::ImageRgb8(src);
    // y = 230 of a 0..255 ramp: high enough to be the shoulder's business,
    // far enough from 255 that clipping is not doing the work.
    let top = |ev: f32| {
        develop_preview(
            &img,
            &EditRecipe { hdr_edit: true, hdr_max_ev: ev, ..Default::default() },
        )
        .to_rgb8()[(32, 230)][0] as i32
    };
    let bare = top(0.0);
    let (d1, d2, d4) = (bare - top(1.0), bare - top(2.0), bare - top(4.0));
    assert!(
        0 < d1 && d1 < d2 && d2 < d4,
        "every extra stop of headroom must pull the top end further: \
         {bare} → −{d1} at 1 EV, −{d2} at 2 EV, −{d4} at 4 EV"
    );
    // The kit's own headroom, and the depth Lightroom was measured to reach
    // there. The limiter held the old model to about 27 on this ramp.
    let stated = bare - top(2.30);
    assert!(
        stated >= 40,
        "at the +2.30 EV the kit states, the shoulder must reach Lightroom's \
         own depth and not a slider limiter's: {bare} → −{stated}"
    );
}

/// v1.5.0 F8: the SDR rendition's Clarity is the Basic panel's Clarity —
/// one operator at one radius, not a second copy.
///
/// The radius rule used to be written twice (`0.02 · min(w,h)`), which is
/// how one of the two would have been tuned and the other left behind.
/// `clarity_radius` is now the single definition, and this is the probe
/// that keeps it single: the same amount through either door has to make
/// the same picture.
///
/// MUTATION: give `hdr::apply` its own radius expression again (any
/// constant but `clarity_radius`'s) and the two renditions diverge.
#[test]
fn the_sdr_renditions_clarity_is_the_basic_panels_clarity() {
    // 512 px, and that size is load-bearing. `clarity_radius` is
    // `0.02·min(w,h)` with a FLOOR of 8, so on the 128 px frame this test
    // first used the rule never bound — 0.02·128 = 2.56 and 0.03·128 =
    // 3.84 both clamp to 8, and a mutated coefficient was invisible
    // (falsification case F8-M7). At 512 the floor is behind us: 10 px
    // against 15.
    let (w, h) = (512u32, 512u32);
    let mut src = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = if (x / 32 + y / 32) % 2 == 0 { 90u8 } else { 165u8 };
            src.put_pixel(x, y, image::Rgb([v, v, v]));
        }
    }
    let img = DynamicImage::ImageRgb8(src);
    let basic = develop_preview(&img, &EditRecipe { clarity: 60.0, ..Default::default() });
    let through_sdr = develop_preview(
        &img,
        &EditRecipe { hdr_edit: true, sdr_clarity: 60.0, ..Default::default() },
    );
    assert_eq!(
        basic.to_rgb8().as_raw(),
        through_sdr.to_rgb8().as_raw(),
        "one Clarity operator at one radius, whichever panel asks for it"
    );
    // …and it is not the identity, so the equality above is not two
    // untouched frames agreeing with each other.
    let bare = develop_preview(&img, &EditRecipe::default());
    assert_ne!(
        bare.to_rgb8().as_raw(),
        basic.to_rgb8().as_raw(),
        "Clarity +60 must actually do something to a checkerboard"
    );
}

/// v1.5.0 F7, the class sweep's SECOND site: a creative profile's
/// per-channel curves reach the pixels, and they compose UNDER the
/// photographer's own.
///
/// This is the arm no real photograph exercises. Adobe writes all three
/// channels of every one of the 161 measured Looks as identity
/// (`0,0 … 255,255`), which `parse_curve_checked` returns EMPTY, so
/// `apply_rgb_curves`'s composition branch has never run on a file. The
/// branch was still wrong — it indexed a 256-entry `curve_lut` with this
/// table's `LUT_N` = 4096 counter, the same out-of-bounds the master
/// curve's arm had — and the 29-mutation sweep is what said so: M27 put
/// the defect back and every one of the 1,593 tests stayed green. A fix
/// nothing can fail is a fix nobody can keep.
///
/// MUTATION: index `base[i]` instead of `sample_lut(&base, x)` (the arm
/// panics); compose the two curves in the other order, or drop either one
/// (the three-way ordering below stops holding).
#[test]
fn a_baked_per_channel_curve_reaches_the_pixels_under_the_photographers_own() {
    use crate::recipe::{CreativeLook, CurvePoint};
    let knot = |input, output| CurvePoint { input, output };
    // RED only, so green is an in-frame control for "did this touch the
    // channels it was not given?".
    //
    // The two curves move the SAME input in opposite directions, and they
    // are deliberately NOT each other's inverse. The first draft made them
    // inverses (128→64 against 64→128) and the mutation that swaps the
    // composition order stayed GREEN: where two curves undo each other,
    // `own(baked(x))` and `baked(own(x))` both land back on x, so the
    // probe was blind to the very order it claimed to pin. These two share
    // the knot at 128 instead, which makes the two orders land on opposite
    // sides of where the pixel started.
    let darker = vec![knot(0, 0), knot(128, 64), knot(255, 255)];
    let lighter = vec![knot(0, 0), knot(128, 192), knot(255, 255)];
    let baked = |c: &[CurvePoint]| CreativeLook {
        name: "per-channel".to_string(),
        amount: 1.0,
        red_curve: c.to_vec(),
        ..Default::default()
    };
    let patch = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, image::Rgb([128; 3])));
    let shot = |r: &EditRecipe| {
        let p = develop_preview(&patch, r).to_rgb8()[(0, 0)];
        (p[0], p[1])
    };

    let (bare_r, bare_g) = shot(&EditRecipe::default());

    // 1) The PROFILE's curve lands at all, on red alone — the half that
    //    was unreachable before F7 read a Look's per-channel curves.
    let (prof_r, prof_g) =
        shot(&EditRecipe { look: Some(baked(&darker)), ..Default::default() });
    assert!(prof_r < bare_r, "a baked 128→64 must darken red: {bare_r} → {prof_r}");
    assert_eq!(prof_g, bare_g, "…and must leave green alone: {bare_g} → {prof_g}");

    // 2) The PHOTOGRAPHER's own curve, on its own, the other way.
    let (own_r, _) =
        shot(&EditRecipe { red_curve: lighter.clone(), ..Default::default() });
    assert!(own_r > bare_r, "a photographer's 64→128 must lift red: {bare_r} → {own_r}");

    // 3) COMPOSITION, in the ORDER the engine claims: the photographer's
    //    curve reads the PROFILE's output, not the raw pixel.
    //
    //    The profile darkens 128 to ~64; the photographer's curve then
    //    reads 64, which it lifts only part of the way, so the composition
    //    lands BELOW where the pixel started. Swap the two and the
    //    photographer's curve reads 128 and lifts it to ~192, which the
    //    profile then darkens to ~160 — ABOVE where it started. One
    //    ordering assertion separates them, and it is stated against
    //    `bare` rather than as an exact value so it does not rest on where
    //    in the encoding this stage happens to sit.
    let (both_r, both_g) = shot(&EditRecipe {
        look: Some(baked(&darker)),
        red_curve: lighter.clone(),
        ..Default::default()
    });
    assert!(
        prof_r < both_r && both_r < bare_r,
        "the photographer's curve reads the profile's OUTPUT, so the pair lands \
         BELOW bare: darker={prof_r}, composed={both_r}, bare={bare_r}, lighter={own_r}"
    );
    assert_eq!(both_g, bare_g, "green is still untouched: {bare_g} → {both_g}");
}

#[test]
fn camera_base_knots_recovers_the_map_and_identity_is_empty() {
    // neutral = a luma gradient; camera = the SAME frame through a known
    // pointwise lift (x^0.6). The CDF match must recover that map at the
    // interior knots, and a self-match must collapse to empty (no-op).
    let (w, h) = (512u32, 64u32);
    let mut n = RgbImage::new(w, h);
    let mut c = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let t = x as f32 / (w - 1) as f32;
            let v = (t * 255.0).round() as u8;
            n.put_pixel(x, y, image::Rgb([v, v, v]));
            let l = (t.powf(0.6) * 255.0).round() as u8;
            c.put_pixel(x, y, image::Rgb([l, l, l]));
        }
    }
    let n = DynamicImage::ImageRgb8(n);
    let c = DynamicImage::ImageRgb8(c);
    let knots =
        camera_base_knots(&n, &c).expect("512x64 holds 512 blocks of the 64-column grid");
    assert!(!knots.is_empty(), "a real lift must be detected");
    assert_eq!(knots.first(), Some(&[0.0, 0.0]), "black endpoint pinned");
    assert_eq!(knots.last(), Some(&[1.0, 1.0]), "white endpoint pinned");
    for p in &knots {
        if p[0] > 0.05 && p[0] < 0.95 {
            assert!(
                (p[1] - p[0].powf(0.6)).abs() < 0.04,
                "knot {p:?} vs expected {}",
                p[0].powf(0.6)
            );
        }
    }
    assert!(
        camera_base_knots(&n, &n).expect("same pair — still judgeable").is_empty(),
        "identity map → Some(empty) (no base look)"
    );
    // Degenerate input is an INABILITY, not a verdict: the pre-era repair
    // keys on exactly this distinction (None retries later; Some(empty)
    // clears a saved curve and stamps the era).
    let tiny =
        DynamicImage::ImageRgb8(RgbImage::from_pixel(50, 50, image::Rgb([128, 128, 128])));
    assert!(
        camera_base_knots(&tiny, &tiny).is_none(),
        "a picture smaller than the block grid is an inability, not an identity verdict"
    );
}

#[test]
fn base_curve_bridges_histogram_gaps_without_plateaus_or_white_pinning() {
    // Night-street-like pair: luma mass in two bands (0..0.30 and
    // 0.62..0.95) with NOTHING between, both sides the same frame through
    // a known lift (x^0.75). The first estimator planted equal-y knots
    // inside the empty band (→ ~30-level posterised plateaus) and latched
    // its top knots to 1.0 on frames darker than its fixed grid (→ whole
    // upper bands pinned to pure white). Quantile-anchored knots must
    // bridge the gap monotonically and keep the tail off white.
    // 512×64 keeps the sample count above the degenerate-input guard.
    let (w, h) = (512u32, 64u32);
    let mut n = RgbImage::new(w, h);
    let mut c = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let t = x as f32 / (w - 1) as f32;
            let nv = if x % 2 == 0 { 0.30 * t } else { 0.62 + 0.33 * t };
            let cv = nv.powf(0.75).min(1.0);
            n.put_pixel(x, y, image::Rgb([(nv * 255.0).round() as u8; 3]));
            c.put_pixel(x, y, image::Rgb([(cv * 255.0).round() as u8; 3]));
        }
    }
    let knots = camera_base_knots(&DynamicImage::ImageRgb8(n), &DynamicImage::ImageRgb8(c))
        .expect("512x64 holds 512 blocks of the 64-column grid");
    assert!(!knots.is_empty(), "a real lift must be detected");
    // Apply through the real render path over a full ramp and measure.
    let mut ramp = RgbImage::new(256, 1);
    for x in 0..256 {
        ramp.put_pixel(x, 0, image::Rgb([x as u8; 3]));
    }
    let r = EditRecipe { base_curve: knots, ..Default::default() };
    let out = develop_preview(&DynamicImage::ImageRgb8(ramp), &r).to_rgb8();
    let vals: Vec<u8> = (0..256u32).map(|x| out[(x, 0)][0]).collect();
    let (mut longest, mut run) = (1usize, 1usize);
    for i in 1..256 {
        if vals[i] == vals[i - 1] {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 1;
        }
    }
    assert!(longest <= 12, "posterised plateau of {longest} identical output levels");
    assert!(vals[229] < 250, "input 0.9 must not latch to white: {}", vals[229]);
    assert!(vals[250] > vals[235], "the tail keeps rising toward the (1,1) pin");
}

/// v1.2.2: the base-look estimator pairs the rendition against the frame
/// it SHOWS. A 300x200 neutral with dark side strips and a "camera"
/// rendition that is its centred 4:3 crop, pixel for pixel: paired whole,
/// the strips' mass sits on one side of the CDF match only and a curve
/// appears where there is none; paired on the camera's frame the pair is
/// the identity it is. A same-frame pair passes through untouched.
#[test]
fn the_base_look_is_estimated_on_the_frame_the_camera_shows() {
    let neutral = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(300, 200, |x, _| {
        if !(16..283).contains(&x) {
            image::Rgb([20, 20, 20])
        } else {
            let v = (x * 255 / 300) as u8;
            image::Rgb([v, v, v])
        }
    }));
    let camera = neutral.crop_imm(16, 0, 267, 200);
    assert!(
        !camera_base_knots(&neutral, &camera).expect("judgeable").is_empty(),
        "paired whole, the edge strips read as a camera curve"
    );
    let paired = camera_frame_of(&neutral, &camera);
    assert_eq!((paired.width(), paired.height()), (267, 200));
    assert!(
        camera_base_knots(&paired, &camera).expect("judgeable").is_empty(),
        "paired on the frame the camera shows, an identical crop is the identity"
    );
    let same = camera_frame_of(&neutral, &neutral.thumbnail(150, 100));
    assert_eq!((same.width(), same.height()), (300, 200), "a same-frame pair is untouched");
}

/// A night-like frame for the corner question, 600×400: a sky graded left
/// to right over dark ground, as the SENSOR saw it through a lens whose
/// profile lifts the corners by 1.8; the same frame with that lift; and two
/// cameras with one tone (0.85 x), one that drew the sensor's picture and
/// one that drew the lifted one.
fn corner_question() -> (crate::recipe::LensProfile, [DynamicImage; 4]) {
    let (w, h) = (600u32, 400u32);
    let lens = crate::recipe::LensProfile {
        vignette: (0..16).map(|i| 1.0 + 0.8 * i as f32 / 15.0).collect(),
        vignette_on: true,
        ..Default::default()
    };
    let scene = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        let v = if y >= h * 5 / 6 { 0.06 } else { 0.20 + 0.25 * x as f32 / (w - 1) as f32 };
        image::Rgb([(v * 255.0).round() as u8; 3])
    }));
    let falloff = EditRecipe {
        lens_profile: crate::recipe::LensProfile {
            vignette: lens.vignette.iter().map(|g| 1.0 / g).collect(),
            vignette_on: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let neutral = develop_preview(&scene, &falloff);
    let lifted = develop_preview(
        &neutral,
        &EditRecipe { lens_profile: lens.clone(), ..Default::default() },
    );
    let tone = |img: &DynamicImage| {
        let mut out = img.to_rgb8();
        for c in out.iter_mut() {
            *c = (0.85 * *c as f32).round() as u8;
        }
        DynamicImage::ImageRgb8(out)
    };
    let cameras = (tone(&neutral), tone(&lifted));
    (lens, [neutral, lifted, cameras.0, cameras.1])
}

/// 2026-09-21: whether the camera's rendition shows the profile's corner
/// lift is measured on the pair. Both answers, and the reading behind each.
#[test]
fn the_estimation_base_is_the_picture_the_camera_drew() {
    let (lens, [neutral, lifted, drew_the_sensor, drew_the_lift]) = corner_question();
    let bytes = |img: &DynamicImage| img.to_rgb8().into_raw();
    let read = |base: &DynamicImage, camera: &DynamicImage| {
        corner_residual(base, camera).expect("600x400 with an open tone range is judgeable")
    };
    let (as_seen, with_lift) = (read(&neutral, &drew_the_sensor), read(&lifted, &drew_the_sensor));
    assert!(
        as_seen.abs() < 0.002 && with_lift < -0.004,
        "a lift the camera never made predicts brighter corners than it drew: \
         as seen {as_seen}, with the lift {with_lift}"
    );
    assert_eq!(
        bytes(&estimation_base(&neutral, &lens, &drew_the_sensor)),
        bytes(&neutral),
        "the rendition is a tone map of the sensor's picture, so that is the base"
    );
    let (as_seen, with_lift) = (read(&neutral, &drew_the_lift), read(&lifted, &drew_the_lift));
    assert!(
        as_seen > 0.003 && with_lift.abs() < 0.002,
        "a lift the camera did make leaves its corners above the sensor's picture: \
         as seen {as_seen}, with the lift {with_lift}"
    );
    assert_eq!(
        bytes(&estimation_base(&neutral, &lens, &drew_the_lift)),
        bytes(&lifted),
        "the rendition is a tone map of the lifted picture, so that is the base"
    );
    // No profile, no question.
    assert_eq!(
        bytes(&estimation_base(&neutral, &Default::default(), &drew_the_lift)),
        bytes(&neutral)
    );
}

/// What the measurement is FOR. The camera's tone here is 0.85 x and
/// nothing else. Matched against the sensor's picture the estimate says so;
/// matched against the lifted picture — what the estimator did from
/// 2026-08-03 to 2026-09-21 whatever the camera had drawn — the lift comes
/// out as tone, and the control stays in the test so the defect stays
/// visible.
#[test]
fn a_corner_lift_the_camera_never_made_does_not_become_tone() {
    let (lens, [neutral, lifted, drew_the_sensor, _]) = corner_question();
    let miss = |knots: &[[f32; 2]]| {
        knots[1..knots.len() - 1]
            .iter()
            .map(|k| (k[1] - 0.85 * k[0]).abs())
            .fold(0.0f32, f32::max)
    };
    let look = camera_base_look(&neutral, &lens, &drew_the_sensor).expect("judgeable");
    assert!(look.len() > 2, "0.85 x is not the identity: {look:?}");
    assert!(miss(&look) < 0.01, "the camera's tone, to a level or two: {look:?}");
    let control = camera_base_knots(&lifted, &drew_the_sensor).expect("judgeable");
    assert!(
        miss(&control) > 0.03,
        "premise: the unconditional lift misses the camera's tone by levels: {control:?}"
    );
}

/// The profile's gain is a function of the SENSOR'S radius. A body set to
/// an in-camera aspect shows a centred crop, so the lift is made on the
/// whole frame and the camera's frame is cut afterwards — lifted after the
/// cut, the crop's own corners would be taken for the sensor's.
#[test]
fn the_lift_is_made_on_the_sensor_frame_and_cut_to_the_cameras_afterwards() {
    let (lens, [neutral, lifted, ..]) = corner_question();
    let shown = lifted.crop_imm(33, 0, 533, 400); // the centred 4:3 of 600x400
    let mut camera = shown.to_rgb8();
    for c in camera.iter_mut() {
        *c = (0.85 * *c as f32).round() as u8;
    }
    let base = estimation_base(&neutral, &lens, &DynamicImage::ImageRgb8(camera));
    assert_eq!((base.width(), base.height()), (533, 400), "paired on the camera's frame");
    assert_eq!(
        base.to_rgb8().into_raw(),
        shown.to_rgb8().into_raw(),
        "the whole frame's lift, cut to the camera's frame"
    );
}

/// A pair that cannot say has not claimed the lift: the sensor's picture
/// stays the base.
#[test]
fn a_pair_that_cannot_say_keeps_the_picture_the_sensor_saw() {
    let (lens, [neutral, ..]) = corner_question();
    let blown = DynamicImage::ImageRgb8(RgbImage::from_pixel(600, 400, image::Rgb([255; 3])));
    assert!(corner_residual(&neutral, &blown).is_none(), "every block clipped");
    assert_eq!(
        estimation_base(&neutral, &lens, &blown).to_rgb8().into_raw(),
        neutral.to_rgb8().into_raw()
    );
    let tiny = neutral.thumbnail(48, 32);
    assert!(corner_residual(&tiny, &tiny).is_none(), "smaller than the grid");
}

/// The three open paths enter through `camera_base_look` and nowhere else,
/// so the pairing cannot drift between them again (v1.2.2's frame pairing
/// reached one of the three), and the entry pairs the frame itself.
#[test]
fn the_three_open_paths_share_one_base_look_entry() {
    for (name, text) in [
        ("pipeline.rs", include_str!("../pipeline.rs")),
        ("serve.rs", include_str!("../serve.rs")),
        ("bin/gui/actions.rs", include_str!("../bin/gui/actions.rs")),
    ] {
        assert!(text.contains("camera_base_look("), "{name} estimates through the one entry");
        for by_hand in ["camera_base_knots(", "camera_frame_of(", "estimation_base("] {
            assert!(!text.contains(by_hand), "{name} assembles the pair by hand: {by_hand}");
        }
    }
    // The v1.2.2 construction, through the entry: a rendition that is the
    // centred 4:3 crop of the neutral, pixel for pixel, is the identity.
    let neutral = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(300, 200, |x, _| {
        if !(16..283).contains(&x) {
            image::Rgb([20, 20, 20])
        } else {
            let v = (x * 255 / 300) as u8;
            image::Rgb([v, v, v])
        }
    }));
    let camera = neutral.crop_imm(16, 0, 267, 200);
    assert!(
        camera_base_look(&neutral, &Default::default(), &camera).expect("judgeable").is_empty(),
        "paired on the frame the camera shows, an identical crop is the identity"
    );
}

#[test]
fn camera_base_knots_merges_same_bin_quantiles_with_a_mean() {
    // A posterised neutral (one constant tone) against a camera side whose
    // mass splits across two levels: every probability lands on the same
    // neutral bin, and keeping only the first duplicate would map that
    // tone to the LOWER camera level. The merged knot must sit between.
    let (w, h) = (512u32, 64u32);
    let mut n = RgbImage::new(w, h);
    let mut c = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            n.put_pixel(x, y, image::Rgb([77; 3])); // luma 0.302
            let cv = if x % 2 == 0 { 128 } else { 179 }; // 0.502 / 0.702
            c.put_pixel(x, y, image::Rgb([cv; 3]));
        }
    }
    let knots = camera_base_knots(&DynamicImage::ImageRgb8(n), &DynamicImage::ImageRgb8(c))
        .expect("512x64 clears the degenerate-input guard");
    assert_eq!(knots.len(), 3, "one merged mid knot between the pins: {knots:?}");
    let mid = knots[1];
    assert!((mid[0] - 0.302).abs() < 0.01, "x = the neutral spike: {mid:?}");
    assert!(
        mid[1] > 0.52 && mid[1] < 0.68,
        "y = a mean over the camera split, not its floor: {mid:?}"
    );
}

/// Real-machine probe, never run in CI: point AUTOSHADE_PROBE_RAW at an
/// ARW and run with `--ignored` to check the whole base-look chain on a
/// real photo — estimator knots + the luma median of the base-curved
/// render vs the camera's own preview (they must land close).
#[test]
#[ignore = "real-machine probe: set AUTOSHADE_PROBE_RAW to an ARW path"]
fn probe_real_raw_base_look() {
    let Some(raw) = crate::config::live_env("AUTOSHADE_PROBE_RAW") else {
        panic!("set AUTOSHADE_PROBE_RAW to a RAW path");
    };
    let raw = std::path::PathBuf::from(raw);
    let cam_probe = crate::decode::embedded_preview(&raw);
    println!(
        "embedded_preview: {:?}",
        cam_probe.as_ref().map(|o| o.as_ref().map(|i| (i.width(), i.height())))
    );
    let knots = crate::pipeline::photo_base_knots(&raw);
    println!("knots: {knots:?}");
    // The corner question on this photo, both readings and both answers.
    if let Ok(Some(cam)) = &cam_probe {
        let lens = crate::pipeline::fresh_lens_profile(&raw);
        let working =
            render_to_image(&raw, &EditRecipe::default(), None, Some(2048)).unwrap();
        let small = working.thumbnail(1024, 1024);
        let plain = camera_frame_of(&small, cam);
        println!("profile corner gain: {:?}", lens.vignette.last());
        println!("corner residual as the sensor saw it: {:?}", corner_residual(&plain, cam));
        if lens.vignette_active() {
            let lift = EditRecipe {
                lens_profile: crate::recipe::LensProfile {
                    vignette: lens.vignette.clone(),
                    vignette_on: true,
                    ..Default::default()
                },
                ..Default::default()
            };
            let lifted = camera_frame_of(&develop_preview(&small, &lift), cam);
            println!("corner residual with the lift: {:?}", corner_residual(&lifted, cam));
            println!("knots as the sensor saw it: {:?}", camera_base_knots(&plain, cam));
            println!("knots with the lift: {:?}", camera_base_knots(&lifted, cam));
            // How far each estimate's render of the SENSOR'S picture sits
            // from the camera's, block by block, in 8-bit levels.
            for (name, base) in [("as seen", &plain), ("with the lift", &lifted)] {
                let Some(knots) = camera_base_knots(base, cam) else { continue };
                let based = develop_preview(
                    &plain,
                    &EditRecipe { base_curve: knots, ..Default::default() },
                );
                let rows = (64.0 * plain.height() as f32 / plain.width() as f32).round() as usize;
                let (ours, theirs) = (
                    block_lumas(&based, 64, rows).unwrap(),
                    block_lumas(cam, 64, rows).unwrap(),
                );
                let mut err: Vec<f32> =
                    ours.iter().zip(&theirs).map(|(a, b)| (a - b) * 255.0).collect();
                let rms = (err.iter().map(|e| e * e).sum::<f32>() / err.len() as f32).sqrt();
                err.sort_by(|a, b| a.total_cmp(b));
                println!(
                    "render with the estimate made {name} vs camera: rms {rms:.2} levels, median {:+.2}",
                    err[err.len() / 2]
                );
            }
        }
    }
    assert!(!knots.is_empty(), "expected a base look on a camera RAW");
    let neutral =
        render_to_image(&raw, &EditRecipe::default(), None, None).unwrap().thumbnail(1536, 1536);
    let based = develop_preview(
        &neutral,
        &EditRecipe { base_curve: knots, ..Default::default() },
    );
    let cam = crate::decode::embedded_preview(&raw).unwrap().unwrap();
    let median = |img: &DynamicImage| -> f32 {
        let rgb = img.to_rgb8();
        let mut v: Vec<f32> = rgb
            .as_raw()
            .chunks(3)
            .map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32)
            .collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2] / 255.0
    };
    let (mn, mb, mc) = (median(&neutral), median(&based), median(&cam));
    println!("median neutral={mn:.3} based={mb:.3} camera={mc:.3}");
    assert!(
        (mb - mc).abs() < 0.06,
        "base-curved render should sit near the camera preview: based={mb:.3} camera={mc:.3}"
    );
}

#[test]
fn the_crop_rectangle_is_one_rule_for_both_source_paths() {
    use crate::recipe::Crop;
    // `apply_crop` exists BECAUSE the RAW path and the baked path must
    // agree on the rectangle — but nothing pinned that rule, so the shared
    // helper's arithmetic was verified by reading only. Pin it three ways.
    //
    // (a) The exact rectangle. Origin from left/top, SIZE from the width
    // and height — an implementation that computed the size from the
    // right/bottom EDGES instead (a natural slip) yields 80x40 here, not
    // 60x30, and a swapped x/y yields a 30-tall crop starting at x=20.
    let img =
        DynamicImage::ImageRgb8(RgbImage::from_fn(100, 50, |x, y| {
            image::Rgb([x as u8, y as u8, 0])
        }));
    let c = Crop { left: 0.2, top: 0.1, right: 0.8, bottom: 0.7 };
    let out = apply_crop(img.clone(), Some(&c)).to_rgb8();
    assert_eq!(out.dimensions(), (60, 30), "size comes from the crop's extent");
    assert_eq!(out.get_pixel(0, 0).0, [20, 5, 0], "origin = (left, top) of the frame");

    // (b) Degenerate and absent rectangles are no-ops, never a zero-size
    // image (a zero-size frame reaches par_chunks_mut(0) downstream).
    let dims = |i: DynamicImage| (i.width(), i.height());
    assert_eq!(dims(apply_crop(img.clone(), None)), (100, 50));
    let flat = Crop { left: 0.5, top: 0.1, right: 0.5, bottom: 0.9 };
    assert_eq!(dims(apply_crop(img.clone(), Some(&flat))), (100, 50));
    // Out-of-range components clamp instead of overflowing the cast.
    let wild = Crop { left: -1.0, top: -1.0, right: 2.0, bottom: 2.0 };
    assert_eq!(dims(apply_crop(img.clone(), Some(&wild))), (100, 50));

    // (c) End to end through the REAL baked pipeline (`render_to_file`
    // dispatches a non-RAW source to `render_baked_to_image`): the
    // deliverable's dimensions must equal what the helper predicts, or the
    // shared rule is not the rule the export actually applies.
    let dir = std::env::temp_dir().join(format!("autoshade-crop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("src.png");
    img.save(&src).unwrap();
    let out_p = dir.join("cropped.png");
    let r = EditRecipe { crop: Some(c), ..Default::default() };
    let (w, h) = render_to_file(&src, &r, &out_p, None, None, crate::diag::stderr()).unwrap();
    assert_eq!((w, h), (60, 30), "the baked export applies the SAME rectangle");
    assert_eq!(image::image_dimensions(&out_p).unwrap(), (60, 30));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_publishes_atomically_and_leaves_no_staging_file() {
    let dir = std::env::temp_dir().join(format!("autoshade-export-atomic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("src.png");
    DynamicImage::ImageRgb8(image::RgbImage::new(4, 3)).save(&src).unwrap();
    let out = dir.join("shot.developed.png");
    let r = EditRecipe::default();
    render_to_file(&src, &r, &out, None, None, crate::diag::stderr()).unwrap();
    assert!(out.exists(), "the deliverable must be published");
    // The staged copy is consumed on EVERY path — a leftover would mean a
    // partial file could survive beside a delivery.
    let residue: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp."))
        .collect();
    assert!(residue.is_empty(), "staging residue left behind: {residue:?}");
    // A re-export replaces it in place, still with no residue.
    render_to_file(&src, &r, &out, None, None, crate::diag::stderr()).unwrap();
    assert!(out.exists());

    // A PRE-STAGING failure: an unknown extension is rejected at format
    // resolution before any file is created — the target must survive
    // and no staging litter may appear. (The old comment claimed this
    // failed "after staging"; it never did — the REAL post-staging case
    // follows below, R12.)
    let keeper = dir.join("keeper.unknownext");
    std::fs::write(&keeper, b"a previous deliverable").unwrap();
    let err = render_to_file(&src, &r, &keeper, None, None, crate::diag::stderr());
    assert!(err.is_err(), "an unknown extension must fail the export");
    assert_eq!(
        std::fs::read(&keeper).unwrap(),
        b"a previous deliverable",
        "a failed export must not touch the file it was going to replace"
    );
    let residue: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp."))
        .collect();
    assert!(residue.is_empty(), "a failed export must clean its staging file: {residue:?}");

    // THE ATOMICITY PROPERTY, exercised after staging actually began: a
    // read-only target makes the PUBLISH rename fail on Windows, so the
    // encode has succeeded and the staging file exists at the moment of
    // failure — the previous deliverable must survive byte-for-byte and
    // the staging file must be cleaned (R12; the old failure aborted
    // before any file handle was opened, so atomicity was never tested).
    #[cfg(windows)]
    {
        let ro = dir.join("keeper.png");
        std::fs::write(&ro, b"a previous deliverable").unwrap();
        let mut perm = std::fs::metadata(&ro).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&ro, perm.clone()).unwrap();
        let err = render_to_file(&src, &r, &ro, None, None, crate::diag::stderr());
        assert!(err.is_err(), "publishing over a read-only file must fail");
        assert_eq!(
            std::fs::read(&ro).unwrap(),
            b"a previous deliverable",
            "a failed PUBLISH must leave the previous deliverable untouched"
        );
        let residue: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(residue.is_empty(), "publish failure must clean staging: {residue:?}");
        // This whole block is #[cfg(windows)], so the lint's "world
        // writable on Unix" concern cannot apply — we are only restoring
        // writability so the temp dir can be removed.
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        std::fs::set_permissions(&ro, perm).unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bitmap_mask_sampling_matches_the_producers_convention() {
    // Producers normalise at PIXEL CENTRES, `(x + MASK_SAMPLE_CENTRE)/w`;
    // the sampler must read on the matching TEXEL-CENTRE grid, or a
    // frame-sized mask loses its last row/column, its placement drifts
    // with resolution, and the whole family sits half a texel out.
    let mut m = image::GrayImage::new(2, 1);
    m.put_pixel(0, 0, image::Luma([0]));
    m.put_pixel(1, 0, image::Luma([255]));
    // The two pixel positions a 2-wide FRAME produces — 0.5/2 and 1.5/2 —
    // are exactly the two TEXEL centres, so both are exact hits with no
    // interpolation: sx = nx·2 − 0.5 = 0 and 1.
    assert_eq!(sample_gray_norm(&m, 0.5 / 2.0, 0.0), 0.0);
    assert_eq!(sample_gray_norm(&m, 1.5 / 2.0, 0.0), 1.0, "last texel must be reachable");
    // Resolution independence: an 8-wide frame over the same 2-wide mask.
    // Pixel 7 sits at nx = 7.5/8 → sx = 1.875 − 0.5 = 1.375, past the last
    // texel centre → clamp-to-edge → full coverage. Pixel 0 sits at
    // nx = 0.5/8 → sx = −0.375 → clamp → nothing.
    assert_eq!(sample_gray_norm(&m, 7.5 / 8.0, 0.0), 1.0);
    assert_eq!(sample_gray_norm(&m, 0.5 / 8.0, 0.0), 0.0);
    // …and the interpolated interior is SYMMETRIC about the frame centre,
    // which is the half-pixel convention made visible on this arm. Every
    // value here is dyadic, so the arithmetic is exact in f32:
    //   pixel 3 → nx = 3.5/8 = 0.4375 → sx = 0.875 − 0.5 = 0.375 → 0.375
    //   pixel 4 → nx = 4.5/8 = 0.5625 → sx = 1.125 − 0.5 = 0.625 → 0.625
    // Under the refuted `x/w` reading they are 0.75 and 1.0 — a pair that
    // is neither symmetric nor even distinct at the top end.
    let p3 = sample_gray_norm(&m, 3.5 / 8.0, 0.0);
    let p4 = sample_gray_norm(&m, 4.5 / 8.0, 0.0);
    assert_eq!(p3, 0.375);
    assert_eq!(p4, 0.625);
    assert_eq!(p3 + p4, 1.0, "the ramp must be symmetric about the frame centre");
}

/// **The half-pixel convention itself, on every arm of the family that can
/// carry one** — the R29 C2 pin, and the one test built so that reverting
/// [`MASK_SAMPLE_CENTRE`] to 0 fails it four separate ways.
///
/// The measurement is in that constant's doc (two Lightroom captures, both
/// putting a nominally centred radial's centre at pixel-index 3119.5 on a
/// 6240-wide frame). What is pinned HERE is that the engine's own arms all
/// implement it, and each fixture is chosen so the two readings disagree by
/// a whole feature rather than by a rounding:
///
/// | arm | fixture | at pixel centres | at `x/w` |
/// |---|---|---|---|
/// | Radial | ellipse = the middle half of a 4 × 4 frame | a centred 2 × 2 block | ONE off-centre pixel (measured) |
/// | Linear | ramp from row 0's centre to row 3's centre | 0, ⅓, ⅔, 1 | 0, ⅙, ½, ⅚ — never reaches full |
/// | Bitmap | a 2-wide raster over a 4-wide frame | 0, ¼, ¾, 1 (symmetric) | 0, ½, 1, 1 (saturates early) |
/// | Brush | one dab at `d 0.5 0.5` on a 16 × 16 frame | mirror-symmetric alpha | the dab lands ON texel 8 |
///
/// Both frame producers are exercised: [`mask_coverage`]'s loop reads the
/// weights directly, and `apply_masks`' own `weight_at` is pinned through a
/// real develop at the end — they are separate lines of code and a mutation
/// of either one alone must be caught.
#[test]
fn every_mask_family_samples_at_pixel_centres() {
    use crate::recipe::{EditRecipe, LocalAdjustment, MaskGeometry};
    let flat = |n: u32| {
        DynamicImage::ImageRgb8(image::RgbImage::from_pixel(n, n, image::Rgb([128, 128, 128])))
    };

    // --- (a) RADIAL -----------------------------------------------------
    // Ellipse centred at (0.5, 0.5) with rx = ry = 0.25, hard edge. On a
    // 4 × 4 frame the pixel centres are nx ∈ {0.125, 0.375, 0.625, 0.875},
    // so (nx − 0.5)/0.25 ∈ {−1.5, −0.5, +0.5, +1.5} and
    // d² ∈ {0.5, 2.5, 4.5}: the four pixels with d = √0.5 = 0.707 are
    // inside and the twelve with d ≥ 1.58 are outside. A centred 2 × 2
    // block, and `radial_falloff(0, d)` is the exact step `d < 1`, so the
    // coverage bytes are exactly 255 and 0 with nothing to round.
    let ell = LocalAdjustment {
        mask: MaskGeometry::Radial {
            top: 0.25,
            left: 0.25,
            bottom: 0.75,
            right: 0.75,
            feather: 0.0,
            roundness: 0.0,
            flipped: false,
            angle: 0.0,
            midpoint: 50.0,
            mask_version: 2,
        },
        ..Default::default()
    };
    let cov = mask_coverage(&ell, &flat(4), MaskFrame::AsRendered);
    let inside: Vec<(u32, u32)> = (0..4)
        .flat_map(|y| (0..4).map(move |x| (x, y)))
        .filter(|&(x, y)| cov.get_pixel(x, y)[0] == 255)
        .collect();
    // At the refuted `x/w` the offsets are {−2, −1, 0, +1} instead, so
    // d² ∈ {0, 1, 2, …} and the four neighbours land at d = 1 EXACTLY,
    // which the strict `d < 1` hard edge excludes: the whole mask collapses
    // to the single pixel (2, 2). Measured, not predicted — reverting
    // `MASK_SAMPLE_CENTRE` prints `left: [(2, 2)]` here.
    assert_eq!(
        inside,
        vec![(1, 1), (2, 1), (1, 2), (2, 2)],
        "a centred ellipse must cover a CENTRED block, not one corner-anchored pixel"
    );
    for y in 0..4 {
        for x in 0..4 {
            let v = cov.get_pixel(x, y)[0];
            assert!(v == 0 || v == 255, "a hard edge has no partial texel: ({x},{y}) = {v}");
        }
    }

    // --- (b) LINEAR -----------------------------------------------------
    // Zero end on row 0's centre (ny = 0.125), full end on row 3's centre
    // (ny = 0.875). vy = 0.75, len2 = 0.5625, so the weights are
    // (ny − 0.125)·0.75/0.5625 = 0, 1/3, 2/3, 1 — every operand dyadic, and
    // the last one EXACTLY 1. Through the shipped `Measured` profile,
    // smoothstep(t^1.124), they are 0, round(52.17) = 52,
    // round(177.52) = 178, 255.
    let ramp = LocalAdjustment {
        mask: MaskGeometry::Linear {
            zero_x: 0.5, zero_y: 0.125, full_x: 0.5, full_y: 0.875,
        },
        ..Default::default()
    };
    let lcov = mask_coverage(&ramp, &flat(4), MaskFrame::AsRendered);
    let column: Vec<u8> = (0..4).map(|y| lcov.get_pixel(2, y)[0]).collect();
    assert_eq!(column, vec![0, 52, 178, 255], "the shipped ramp must span its ends exactly");

    // --- (c) BITMAP -----------------------------------------------------
    // A 2-wide raster [0, 255] read by a 4-wide frame. Texel centres sit at
    // nx = 0.25 and 0.75; the frame's pixel centres at 0.125/0.375/0.625/
    // 0.875 give sx = nx·2 − 0.5 = −0.25, 0.25, 0.75, 1.25, which clamp to
    // 0, 0.25, 0.75, 1 — symmetric about the frame centre, and reaching
    // BOTH ends. The `Bitmap` arm ignores its path when a raster is handed
    // in, so this needs no file.
    let mut ras = image::GrayImage::new(2, 1);
    ras.put_pixel(0, 0, image::Luma([0]));
    ras.put_pixel(1, 0, image::Luma([255]));
    let bmp = MaskGeometry::Bitmap { path: "unused — the raster is passed in".into() };
    let row: Vec<f32> = (0..4u32)
        .map(|x| {
            mask_weight(&bmp, (x as f32 + MASK_SAMPLE_CENTRE) / 4.0, 0.5, Some(&ras))
        })
        .collect();
    assert_eq!(row, vec![0.0, 0.25, 0.75, 1.0]);
    assert_eq!(row[0] + row[3], 1.0, "the two ends must mirror");
    assert_eq!(row[1] + row[2], 1.0, "…and so must the interior");

    // --- (d) BRUSH ------------------------------------------------------
    // One dab at the exact frame centre of a 16 × 16 frame. `rasterise_
    // brush_group` stamps it at texel coordinate 0.5·16 − 0.5 = 7.5, i.e.
    // BETWEEN texels 7 and 8, so the alpha is mirror-symmetric about the
    // frame centre; the frame then reads texel x exactly (16 is a power of
    // two, so (x + 0.5)/16 · 16 − 0.5 = x with no rounding at all). At
    // `x/w` the dab would land ON texel 8 and the mirror would break —
    // texel 4 falls exactly on ρ = 1 and reads 0 while its partner texel 11
    // is still lit.
    let dab = probe_brush(&[(1.0, 0.25, 1.0, 0.5, "d 0.5 0.5")]);
    let braster = brush_raster(&dab, 16, 16).expect("one dab");
    assert_eq!(braster.dimensions(), (16, 16), "small frame = 1:1 raster");
    let at = |x: u32, y: u32| {
        mask_weight(
            &dab,
            (x as f32 + MASK_SAMPLE_CENTRE) / 16.0,
            (y as f32 + MASK_SAMPLE_CENTRE) / 16.0,
            Some(&braster),
        )
    };
    for x in 0..16u32 {
        assert_eq!(at(x, 7), at(15 - x, 7), "the dab is not mirrored in x at column {x}");
        assert_eq!(at(7, x), at(7, 15 - x), "the dab is not mirrored in y at row {x}");
    }
    // The mirror is only meaningful if the dab is actually THERE and the
    // pair straddling the centre share the peak (a single peak texel is the
    // `x/w` signature).
    assert!(at(7, 7) > 0.9, "premise: the dab covers the centre: {}", at(7, 7));
    assert_eq!(at(7, 7), at(8, 8), "the centre must be shared, not owned by one texel");
    assert_eq!(at(0, 7), 0.0, "…and the dab still ends: {}", at(0, 7));

    // --- both frame producers -------------------------------------------
    // `mask_coverage` above is the OVERLAY's loop. `apply_masks`' own
    // `weight_at` is a separate line, so pin it on the same radial: exactly
    // the centred 2 × 2 may move.
    let r = EditRecipe {
        masks: vec![LocalAdjustment { exposure_ev: -4.0, ..ell }],
        ..Default::default()
    };
    let mut data = vec![[0.6_f32; 3]; 16];
    apply_develop_anon(&mut data, 4, 4, &r);
    let moved: Vec<usize> =
        (0..16).filter(|&i| (data[i][0] - 0.6).abs() > 1e-4).collect();
    assert_eq!(
        moved,
        vec![5, 6, 9, 10],
        "the render's own producer must agree with the overlay's"
    );
}

#[test]
fn straighten_survives_a_zero_size_frame() {
    let img = DynamicImage::ImageRgb8(image::RgbImage::new(0, 0));
    let out = rotate_straighten(&img, 5.0);
    assert_eq!((out.width(), out.height()), (0, 0));
}

#[test]
fn develop_survives_a_zero_size_frame() {
    // rayon asserts chunk_size != 0 even on an EMPTY slice, so every
    // `par_chunks_mut(w)` in the develop needs a zero-dim guard. Batch 40
    // guarded the two vignette passes and claimed they were the last of
    // the family; `apply_masks` in fact had two more (its tone pass and
    // its local-NR pass), so this recipe carries a MASK as well — the
    // case that still panicked (R12).
    let img = DynamicImage::ImageRgb8(image::RgbImage::new(0, 0));
    let r = EditRecipe {
        lens_vignette: 60.0,
        lens_profile: crate::recipe::LensProfile {
            vignette: vec![1.0, 1.2, 1.4, 1.6],
            vignette_on: true,
            ..Default::default()
        },
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Linear { zero_x: 0.0, zero_y: 0.0, full_x: 1.0, full_y: 1.0 },
            amount: 1.0,
            exposure_ev: 1.0,
            noise_reduction: 50.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let out = develop_preview(&img, &r);
    assert_eq!((out.width(), out.height()), (0, 0));
}

#[test]
fn grading_blending_controls_overlap_not_amplitude() {
    use crate::recipe::ColorGrade;
    // A legal Blending of 0 used to zero every regional wheel. It must
    // now still grade — only the region split gets tighter.
    let mut cg = ColorGrade { blending: 0.0, shadow_sat: 100.0, shadow_hue: 210.0, ..Default::default() };
    let mut dark = [[0.08f32, 0.08, 0.08]];
    apply_color_grade(&mut dark, &cg);
    assert!(
        (dark[0][2] - dark[0][0]).abs() > 0.01,
        "blending=0 must still apply the shadow wheel, got {:?}",
        dark[0]
    );
    // And 100 must keep the shadow wheel OFF the midtone: at l = 0.5,
    // mid = 0.5, w_sh = 1 − smoothstep(0, 0.5, 0.5) = 0 exactly, and the
    // midtone/global wheels carry no sat/lum here — the pixel must come
    // back UNTOUCHED. (The old form compared two applications with
    // field-for-field identical ColorGrade values — f(x) == f(x) could
    // never fail, so a shadow-into-midtone leak passed, U14.)
    cg.blending = 100.0;
    let mut a = [[0.5f32, 0.5, 0.5]];
    apply_color_grade(&mut a, &cg);
    assert_eq!(
        a[0],
        [0.5f32, 0.5, 0.5],
        "blending=100: the shadow wheel must not reach the midtone"
    );
    // AMPLITUDE INVARIANCE on a fully-owned deep shadow: blending shapes
    // the region SPLIT only, so a pixel every split assigns to the
    // shadow wheel must grade the same at 0 / 50 / 100. An engine that
    // additionally scaled the regional weights by any blending factor
    // passes the two probes above (the deep probe only ran at 0, the
    // midpoint at 100) but fails this sweep (R12). l = 0.02 sits below
    // every sh_start; the b=100 ramp contributes only ~5e-3 of weight
    // there, far under the mutation's 2x amplitude swing.
    let mut sweep = Vec::new();
    for b in [0.0f32, 50.0, 100.0] {
        let mut px = [[0.02f32, 0.02, 0.02]];
        apply_color_grade(
            &mut px,
            &ColorGrade { blending: b, shadow_sat: 100.0, shadow_hue: 210.0, ..Default::default() },
        );
        sweep.push(px[0]);
    }
    // ENDPOINTS, not consecutive pairs. An amplitude multiplier moves
    // this probe monotonically across the sweep, so each STEP is only
    // ~4e-3 — under a 5e-3 tolerance — while end to end it moves ~8e-3.
    // The consecutive form therefore passed on the exact mutation this
    // block names (measured against the real engine, which stays within
    // 3.7e-5 end to end, so the endpoint form keeps 100x+ of headroom).
    for (a, b) in sweep[0].iter().zip(&sweep[sweep.len() - 1]) {
        assert!(
            (a - b).abs() < 5e-3,
            "blending changed the shadow AMPLITUDE: {sweep:?}"
        );
    }
}

#[test]
fn duplicate_curve_points_do_not_cliff() {
    use crate::recipe::CurvePoint;
    // Two outputs at ONE input is not a function; the documented rule is
    // first-point-wins, which must hold at the code AND just after it.
    let lut = curve_lut(&[
        CurvePoint { input: 0, output: 0 },
        CurvePoint { input: 128, output: 200 },
        CurvePoint { input: 128, output: 50 },
        CurvePoint { input: 255, output: 255 },
    ]);
    let step = (lut[129] - lut[128]).abs();
    assert!(step < 0.05, "one-bin cliff at the duplicate: {step} ({} -> {})", lut[128], lut[129]);
    assert!(lut[128] > 0.7, "first point must win at the duplicate code: {}", lut[128]);
}

#[test]
fn white_point_is_invariant_to_highlights_and_bright_stays_bright() {
    // The engine renders faithfully: Highlights shapes the shoulder but must NOT
    // move the white point, so the brightest tone stays pinned at white. (Keeping
    // bright FOAM bright under an over-cooked recipe is the recipe layer's job —
    // EditRecipe::temper — not an engine override.)
    for h in [-100.0, -78.81, -30.0, 30.0, 100.0] {
        let lut = build_tone_lut(&EditRecipe { highlights: h, ..Default::default() });
        assert!(
            (sample_lut(&lut, 1.0) - 1.0).abs() < 1e-3,
            "highlights {h} moved the white point: {}",
            sample_lut(&lut, 1.0)
        );
    }
    // A neutral recipe must leave bright near-white foam bright.
    let mut foam = vec![[0.90_f32, 0.93, 0.96]];
    apply_develop_anon(&mut foam, 1, 1, &EditRecipe::default());
    let lum = 0.299 * foam[0][0] + 0.587 * foam[0][1] + 0.114 * foam[0][2];
    assert!(lum > 0.90, "neutral recipe dimmed bright foam: {lum}");
}

#[test]
fn tempered_recipe_renders_foam_light_and_water_saturated() {
    // End-to-end: the over-cooked AI recipe (the one that greyed the foam),
    // after clamp + temper, rendered through the monotone curve. Foam must be
    // LIGHT (not crushed to the muddy ~0.6 grey it was) and water must stay
    // turquoise — the engine + recipe layers compose, no engine override.
    let mut r = EditRecipe {
        highlights: -78.81,
        shadows: 36.56,
        whites: 10.27,
        blacks: -14.59,
        contrast: 4.68,
        exposure_ev: -0.177,
        vibrance: 11.19,
        saturation: 2.9,
        ..Default::default()
    };
    r.clamp();
    r.temper(crate::recipe::GradeStrength::calibrated());
    let lum = |p: [f32; 3]| 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2];
    let mut foam = vec![[0.90_f32, 0.93, 0.96]];
    apply_develop_anon(&mut foam, 1, 1, &r);
    assert!(lum(foam[0]) > 0.80, "foam crushed (should stay light): luma {}", lum(foam[0]));
    let mut water = vec![[0.35_f32, 0.62, 0.66]];
    apply_develop_anon(&mut water, 1, 1, &r);
    let [rr, gg, bb] = water[0];
    assert!(gg > rr + 0.10 && bb > rr + 0.10, "water lost its turquoise: [{rr}, {gg}, {bb}]");
}

#[test]
fn region_tones_pin_endpoints_and_stay_monotonic() {
    // Highlights/shadows/contrast must never move the endpoints (only whites/
    // blacks may), and the curve must stay monotone under any extreme combo.
    let recipes = [
        EditRecipe::default(),
        EditRecipe { highlights: -100.0, shadows: 100.0, contrast: 100.0, ..Default::default() },
        EditRecipe { highlights: 100.0, shadows: -100.0, contrast: -100.0, ..Default::default() },
    ];
    for r in recipes {
        let lut = build_tone_lut(&r);
        for i in 1..lut.len() {
            assert!(lut[i] >= lut[i - 1] - 1e-6, "non-monotonic at {i}");
        }
        assert!(sample_lut(&lut, 0.0) < 1e-3, "black point moved by hi/sh/contrast");
        assert!((sample_lut(&lut, 1.0) - 1.0).abs() < 1e-3, "white point moved by hi/sh/contrast");
    }
}

/// v1.5.0, the parametric curve's contract (`parametric_lut`): nothing at
/// rest wherever the splits sit, each region slider moving its OWN region
/// the way its sign says and leaving the far end of the range alone, both
/// ends pinned and the curve monotone at every extreme and split layout,
/// composed UNDER the point curve — and reaching the develop.
///
/// MUTATIONS THIS CATCHES: `PARAMETRIC_REACH` above ½ (the extremes fold
/// the curve back), a region's control value moved by the wrong index
/// (the direction table), the composition order swapped (the point-curve
/// case), and `apply_develop`'s `tone_neutral` blind to the curve.
#[test]
fn the_parametric_curve_moves_its_own_region_and_stays_monotone() {
    let base = build_tone_lut(&EditRecipe::default());
    assert!(parametric_lut(&EditRecipe { param_midtone_split: 70.0, ..Default::default() }).is_none());
    for (field, centre, far) in [
        ("param_shadows", 0.125f32, 0.875f32),
        ("param_darks", 0.375, 0.875),
        ("param_lights", 0.625, 0.125),
        ("param_highlights", 0.875, 0.125),
    ] {
        for sign in [1.0f32, -1.0] {
            let mut json = serde_json::to_value(EditRecipe::default()).expect("serialises");
            json[field] = serde_json::json!(100.0 * sign);
            let r: EditRecipe = serde_json::from_value(json).expect("a region value");
            let lut = build_tone_lut(&r);
            let moved = sample_lut(&lut, centre) - sample_lut(&base, centre);
            assert!(moved * sign > 0.03, "{field} {sign:+}: its own centre moved {moved}");
            let stray = (sample_lut(&lut, far) - sample_lut(&base, far)).abs();
            assert!(stray < 0.02, "{field} {sign:+}: the far end of the range moved {stray}");
        }
    }
    // Every extreme, over the split layouts at both walls and the default.
    for splits in [[25.0, 50.0, 75.0], [10.0, 20.0, 30.0], [70.0, 80.0, 90.0], [10.0, 50.0, 90.0]] {
        for signs in 0..16u32 {
            let at = |k: u32| if (signs >> k) & 1 == 1 { 100.0 } else { -100.0 };
            let r = EditRecipe {
                param_shadows: at(0),
                param_darks: at(1),
                param_lights: at(2),
                param_highlights: at(3),
                param_shadow_split: splits[0],
                param_midtone_split: splits[1],
                param_highlight_split: splits[2],
                ..Default::default()
            };
            let lut = parametric_lut(&r).expect("a moved region builds the curve");
            for i in 1..lut.len() {
                assert!(lut[i] >= lut[i - 1] - 1e-6, "{splits:?} signs {signs:04b}: non-monotone at {i}");
            }
            assert!(lut[0].abs() < 1e-6 && (lut[LUT_N - 1] - 1.0).abs() < 1e-6, "{splits:?}: an end moved");
        }
    }
    // Composed UNDER the point curve: the point curve bends what the
    // regions made, and the order is observable on this pair.
    let both = EditRecipe {
        param_darks: 60.0,
        tone_curve: vec![
            crate::recipe::CurvePoint { input: 0, output: 0 },
            crate::recipe::CurvePoint { input: 96, output: 160 },
            crate::recipe::CurvePoint { input: 255, output: 255 },
        ],
        ..Default::default()
    };
    let (p, c, lut) = (parametric_lut(&both).expect("moved"), curve_lut(&both.tone_curve), build_tone_lut(&both));
    let mut observable = 0.0f32;
    for x in [0.2f32, 0.3, 0.4, 0.5] {
        let want = sample_lut(&c, sample_lut(&p, x));
        assert!((sample_lut(&lut, x) - want).abs() < 2e-3, "at {x}: {} vs {want}", sample_lut(&lut, x));
        observable = observable.max((want - sample_lut(&p, sample_lut(&c, x))).abs());
    }
    assert!(observable > 0.02, "premise: the two orders differ here ({observable})");
    // …and the develop runs it: a mid-dark grey brightens under Darks.
    let mut grey = vec![[0.3f32, 0.3, 0.3]];
    apply_develop_anon(&mut grey, 1, 1, &EditRecipe { param_darks: 80.0, ..Default::default() });
    let mut rest = vec![[0.3f32, 0.3, 0.3]];
    apply_develop_anon(&mut rest, 1, 1, &EditRecipe::default());
    assert!(grey[0][1] > rest[0][1] + 0.02, "the develop skipped the curve: {:?} vs {:?}", grey[0], rest[0]);
}

/// v1.5.0, the Calibration panel ([`Calibration`]): a grey is still grey
/// under every primary slider at both extremes, each primary's Hue turns
/// its own colour the way Lightroom's slider track reads (+ red toward
/// yellow, + green toward cyan, + blue toward magenta), Saturation moves
/// that colour's chroma, and the shadows tint pulls a dark grey toward
/// magenta (+) or green (−) while a light grey does not move.
///
/// MUTATIONS THIS CATCHES: the hue rotation's sign flipped, the basis
/// change composed the wrong way round (white stops mapping to white), the
/// tint's luminance weight inverted (highlights tinted), and the stage
/// left out of the develop.
#[test]
fn calibration_turns_its_own_primary_and_keeps_grey_grey() {
    let chroma = |p: [f32; 3]| p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2]);
    let with = |name: &str, v: f32| {
        let mut json = serde_json::to_value(EditRecipe::default()).expect("serialises");
        json[name] = serde_json::json!(v);
        serde_json::from_value::<EditRecipe>(json).expect("a calibration value")
    };
    let dev = |px: [f32; 3], r: &EditRecipe| {
        let mut d = vec![px];
        apply_develop_anon(&mut d, 1, 1, r);
        d[0]
    };
    for name in ["cal_red_hue", "cal_red_sat", "cal_green_hue", "cal_green_sat", "cal_blue_hue", "cal_blue_sat"] {
        for v in [100.0f32, -100.0] {
            for g in [0.2f32, 0.5, 0.9] {
                let out = dev([g, g, g], &with(name, v));
                assert!(
                    chroma(out) < 2e-3 && (out[1] - g).abs() < 2e-3,
                    "{name} {v:+}: grey {g} became {out:?}"
                );
            }
        }
    }
    let (red, green, blue) = ([0.8f32, 0.2, 0.2], [0.2f32, 0.8, 0.2], [0.2f32, 0.2, 0.8]);
    let up = dev(red, &with("cal_red_hue", 100.0));
    let down = dev(red, &with("cal_red_hue", -100.0));
    assert!(up[1] > up[2] + 0.02, "red hue + turns red toward yellow: {up:?}");
    assert!(down[2] > down[1] + 0.02, "red hue − turns red toward magenta: {down:?}");
    let up = dev(green, &with("cal_green_hue", 100.0));
    assert!(up[2] > up[0] + 0.02, "green hue + turns green toward cyan: {up:?}");
    let up = dev(blue, &with("cal_blue_hue", 100.0));
    assert!(up[0] > up[1] + 0.02, "blue hue + turns blue toward magenta: {up:?}");
    assert!(chroma(dev(red, &with("cal_red_sat", 100.0))) > chroma(red) + 0.02);
    assert!(chroma(dev(red, &with("cal_red_sat", -100.0))) < chroma(red) - 0.02);
    let dark = dev([0.15; 3], &with("cal_shadow_tint", 100.0));
    assert!(dark[0] > dark[1] + 0.01 && dark[2] > dark[1] + 0.01, "+ tints the shadows magenta: {dark:?}");
    let dark = dev([0.15; 3], &with("cal_shadow_tint", -100.0));
    assert!(dark[1] > dark[0] + 0.01, "− tints the shadows green: {dark:?}");
    let light = dev([0.9; 3], &with("cal_shadow_tint", 100.0));
    assert!(chroma(light) < 2e-3, "a light grey is not a shadow: {light:?}");
    assert_eq!(dev(red, &EditRecipe::default()), red, "no slider, no stage");
}

/// v1.5.0, the B&W treatment ([`apply_gray_mix`]): every pixel turns grey,
/// a mixer band lightens or darkens its own colour and nothing else, the
/// colour mixer has nothing to act on, a mixer without the treatment is
/// not a treatment, and the toning tools — the colour grade and the
/// per-channel curves — still reach the grey.
///
/// MUTATIONS THIS CATCHES: the conversion placed after the RGB curves (the
/// channel-curve toning vanishes), the band weight read from the wrong
/// band, HSL still applied in black and white, and the mixer applied
/// without the treatment.
#[test]
fn black_and_white_mixes_by_band_and_still_takes_toning() {
    use crate::recipe::{ColorGrade, CurvePoint, Hsl};
    let chroma = |p: &[f32; 3]| p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2]);
    let frame = vec![[0.8f32, 0.2, 0.2], [0.2, 0.3, 0.8], [0.5, 0.5, 0.5], [0.3, 0.7, 0.2]];
    let run = |r: &EditRecipe| {
        let mut d = frame.clone();
        apply_develop_anon(&mut d, 4, 1, r);
        d
    };
    let bw = EditRecipe { convert_to_grayscale: true, ..Default::default() };
    let flat = run(&bw);
    for p in &flat {
        assert!(chroma(p) < 1e-4, "black and white leaves no colour: {flat:?}");
    }
    assert!((flat[2][0] - 0.5).abs() < 2e-3, "a grey keeps its value: {:?}", flat[2]);
    let lighter = run(&EditRecipe { gray_red: 100.0, ..bw.clone() });
    assert!(lighter[0][0] > flat[0][0] + 0.05, "red +100 lightens the red: {lighter:?} vs {flat:?}");
    for i in 1..4 {
        assert!((lighter[i][0] - flat[i][0]).abs() < 1e-4, "red +100 moved pixel {i}: {lighter:?}");
    }
    let darker = run(&EditRecipe { gray_red: -100.0, ..bw.clone() });
    assert!(darker[0][0] < flat[0][0] - 0.05, "red −100 darkens the red: {darker:?}");
    let blue = run(&EditRecipe { gray_blue: 100.0, ..bw.clone() });
    assert!(blue[1][0] > flat[1][0] + 0.05 && (blue[0][0] - flat[0][0]).abs() < 1e-4);
    let hsl = run(&EditRecipe { hsl: Hsl { luminance: [100.0; 8], ..Default::default() }, ..bw.clone() });
    assert_eq!(hsl, flat, "the colour mixer does not act in black and white");
    assert_eq!(run(&EditRecipe { gray_red: 100.0, ..Default::default() }), frame, "a mixer is not a treatment");
    let graded = run(&EditRecipe {
        color_grade: ColorGrade { shadow_hue: 30.0, shadow_sat: 60.0, ..Default::default() },
        ..bw.clone()
    });
    assert!(chroma(&graded[1]) > 0.01, "the grade tones the grey: {graded:?}");
    let curved = run(&EditRecipe {
        red_curve: vec![
            CurvePoint { input: 0, output: 0 },
            CurvePoint { input: 128, output: 160 },
            CurvePoint { input: 255, output: 255 },
        ],
        ..bw.clone()
    });
    assert!(curved[2][0] > curved[2][1] + 0.02, "a red-channel curve tones the grey: {curved:?}");
    let saturated = run(&EditRecipe { saturation: 100.0, ..bw });
    assert_eq!(saturated, flat, "Saturation has no colour to move in black and white");
}

/// v1.5.0, Point Color ([`apply_point_colors`]): a swatch sampled from a
/// colour moves that colour, fades out across its hue window, leaves a
/// colour outside its windows and every grey exactly as they were, and
/// moves nothing at all while its shifts are 0.
///
/// MUTATIONS THIS CATCHES: the hue offset not wrapped (a red swatch
/// missing the red at 359°), the trapezoid's falloff inverted, the chroma
/// gate dropped (greys recoloured), and the eyedropper sampling in a
/// different colour model from the one the render matches in.
#[test]
fn a_point_color_moves_its_own_colour_and_nothing_else() {
    use crate::recipe::PointColor;
    let red = [0.8f32, 0.2, 0.2];
    let swatch = point_color_at(red).expect("a red has a colour to hold");
    let (h, s, l) = rgb_to_hsl(red[0], red[1], red[2]);
    assert!((swatch.src_hue - h * std::f32::consts::TAU).abs() < 1e-5);
    assert_eq!((swatch.src_sat, swatch.src_lum), (s, l));
    let chip = point_color_rgb(&swatch);
    assert!(chip.iter().zip(red).all(|(a, b)| (a - b).abs() < 1e-4), "the chip is the sample: {chip:?}");
    assert!(point_color_at([0.5, 0.5, 0.5]).is_none(), "a grey is nobody's colour");
    assert!(point_color_at([0.52, 0.5, 0.5]).is_none(), "nor is a colour the gate gives to nobody");
    // red, blue, grey, orange (partly inside the hue window), and a red on
    // the far side of 0° (hue 355°).
    let frame = vec![red, [0.2, 0.3, 0.8], [0.5, 0.5, 0.5], [0.8, 0.53, 0.2], [0.8, 0.2, 0.25]];
    let run = |p: PointColor| {
        let mut d = frame.clone();
        apply_develop_anon(&mut d, 5, 1, &EditRecipe { point_colors: vec![p], ..Default::default() });
        d
    };
    assert_eq!(run(swatch.clone()), frame, "a swatch with no shift moves nothing");
    let turned = run(PointColor { hue_shift: 1.0, ..swatch.clone() });
    let hue_of = |p: [f32; 3]| rgb_to_hsl(p[0], p[1], p[2]).0;
    let moved = |i: usize| (hue_of(turned[i]) - hue_of(frame[i]) + 0.5).rem_euclid(1.0) - 0.5;
    assert!((moved(0) - POINT_HUE_SHIFT_TURNS).abs() < 0.01, "the sampled red turns fully: {}", moved(0));
    assert!(moved(3) > 0.005 && moved(3) < moved(0) - 0.005, "orange turns partly: {}", moved(3));
    assert!(moved(4) > 0.02, "a red across 0° is still the swatch's red: {}", moved(4));
    assert_eq!(turned[1], frame[1], "blue is outside the window");
    assert_eq!(turned[2], frame[2], "a grey has no colour to match");
    let greyed = run(PointColor { sat_scale: -1.0, ..swatch.clone() });
    assert!(greyed[0][0] - greyed[0][2] < 0.02, "saturation −1 greys the red: {:?}", greyed[0]);
    let lifted = run(PointColor { lum_scale: 1.0, ..swatch });
    assert!(luma601(&lifted[0]) > luma601(&frame[0]) + 0.05, "luminance +1 lifts the red: {:?}", lifted[0]);
}

/// The Point Color eyedropper's reference develop: what the stages AFTER
/// the point colours hold changes nothing in it, and what the stages before
/// them hold still does.
///
/// MUTATION THIS CATCHES: a downstream stage left on in
/// `point_color_sampling_recipe` (the swatch lands on the graded or
/// saturated colour, outside the windows the render then tests), or an
/// upstream one switched off with them (the swatch misses the colour the
/// mixer made).
#[test]
fn the_point_color_eyedropper_samples_the_frame_the_point_colours_see() {
    use crate::recipe::{ColorGrade, Hsl, PointColor};
    let frame: Vec<[f32; 3]> = (0..64)
        .map(|i| {
            let t = i as f32 / 63.0;
            [0.2 + 0.6 * t, 0.5 - 0.3 * t, 0.3 + 0.2 * (t * 7.0).sin().abs()]
        })
        .collect();
    let sample = |r: &EditRecipe| {
        let mut d = frame.clone();
        apply_develop_anon(&mut d, 8, 8, &point_color_sampling_recipe(r));
        d
    };
    let upstream = EditRecipe { exposure_ev: 0.3, hsl: Hsl { hue: [40.0; 8], ..Default::default() }, ..Default::default() };
    let quiet = sample(&upstream);
    let downstream = EditRecipe {
        color_grade: ColorGrade { shadow_hue: 200.0, shadow_sat: 80.0, ..Default::default() },
        clarity: 60.0,
        texture: 40.0,
        saturation: 70.0,
        vibrance: -50.0,
        color_nr: 40.0,
        noise_reduction: 40.0,
        sharpening: 90.0,
        point_colors: vec![PointColor { hue_shift: 1.0, ..PointColor::sampled(1.0, 0.6, 0.5) }],
        ..upstream.clone()
    };
    assert_eq!(sample(&downstream), quiet, "the stages after the point colours are not in the sample");
    assert_ne!(sample(&EditRecipe::default()), quiet, "the stages before them are");
}

/// A slider must run out of authority, never destroy detail.
///
/// Monotonicity and pinned endpoints — the two things the tone tests
/// asserted for four rounds — are both TRUE of a perfectly flat band, so
/// they were blind to the worst thing this curve can do. Measured on the
/// pre-fix engine through the real export path: `whites: -50` mapped input
/// 0.9568–0.9731 to one 16-bit code and left the top decade with 75
/// distinct codes out of 411; `highlights: +60` mapped everything above
/// 0.8195 to pure white. Both are ordinary edits.
///
/// So this pins the property those tests missed: no input band survives
/// the curve as a single output value.
#[test]
fn no_slider_setting_collapses_a_tonal_band() {
    const N: usize = 4096;
    // The onset of the old collapse for each slider, and past it.
    let recipes = [
        ("neutral", EditRecipe::default()),
        ("whites -50", EditRecipe { whites: -50.0, ..Default::default() }),
        ("whites -100", EditRecipe { whites: -100.0, ..Default::default() }),
        ("highlights +60", EditRecipe { highlights: 60.0, ..Default::default() }),
        ("highlights +100", EditRecipe { highlights: 100.0, ..Default::default() }),
        ("blacks +60", EditRecipe { blacks: 60.0, ..Default::default() }),
        ("blacks +100", EditRecipe { blacks: 100.0, ..Default::default() }),
        ("shadows +76", EditRecipe { shadows: 76.0, ..Default::default() }),
        ("shadows +100", EditRecipe { shadows: 100.0, ..Default::default() }),
        ("contrast +100", EditRecipe { contrast: 100.0, ..Default::default() }),
        (
            "the old extreme combo",
            EditRecipe { highlights: -100.0, shadows: 100.0, contrast: 100.0, ..Default::default() },
        ),
        (
            "everything at once",
            EditRecipe {
                whites: -100.0,
                blacks: 100.0,
                highlights: 100.0,
                shadows: 100.0,
                contrast: 100.0,
                ..Default::default()
            },
        ),
    ];
    // A run this long is a visibly flat patch, not quantisation: the worst
    // measured pre-fix run was 740 of 4096 and the neutral curve's is 1.
    const MAX_RUN: usize = 96;
    for (name, r) in recipes {
        let lut = build_tone_lut(&r);
        let out: Vec<u16> = (0..N)
            .map(|i| {
                let x = i as f32 / (N - 1) as f32;
                (sample_lut(&lut, x).clamp(0.0, 1.0) * 65535.0).round() as u16
            })
            .collect();
        let (mut run, mut worst, mut worst_at) = (1usize, 1usize, 0usize);
        for i in 1..out.len() {
            run = if out[i] == out[i - 1] { run + 1 } else { 1 };
            if run > worst {
                worst = run;
                worst_at = i;
            }
        }
        assert!(
            worst <= MAX_RUN,
            "{name}: {worst} consecutive inputs (around x={:.4}) all render to {} — \
             a slider flattened a tonal band instead of saturating",
            worst_at as f32 / (N - 1) as f32,
            out[worst_at]
        );
    }
}

/// The same property with EXPOSURE in play — the dimension the test above
/// never varies, and the one where this design's guarantee actually ends.
///
/// Two corrections to how the band is measured, both from re-deriving the
/// numbers rather than trusting the earlier write-up:
///
///   * A run at 0 or 65535 is CLIPPING, which is what a strong slider on
///     an already-bright frame is supposed to do; a run at an interior
///     code is destroyed detail. Counting both together made
///     `contrast: +100` at `+0.5 EV` look like a 161-input collapse when
///     every one of those inputs renders to pure white — and the same
///     measurement said the pre-fix `highlights: +60` was harmless, which
///     it was not. Only interior runs are counted here.
///   * Measured that way, the v0.18.0 limiter's win is still real and
///     larger than it looked: `whites: -50` goes from a 157-input
///     interior plateau (no limiter) to 43.
///
/// The threshold is what the shipped design HOLDS, not what would be
/// nice — see the measured-grid note ahead of the loop below.
#[test]
fn no_slider_collapses_an_interior_band_at_any_exposure() {
    const N: usize = 4096;
    // What the shipped design HOLDS on this WHOLE grid, with margin: the
    // worst cell measures 100 (weighted-knot model + per-slider λ). Not a
    // wish — measured.
    const MAX_RUN: usize = 128;
    type Case = (&'static str, fn(&mut EditRecipe));
    let sliders: [Case; 18] = [
        ("neutral", |_r| {}),
        ("whites -50", |r| r.whites = -50.0),
        ("whites -100", |r| r.whites = -100.0),
        ("highlights +60", |r| r.highlights = 60.0),
        ("highlights +100", |r| r.highlights = 100.0),
        ("blacks +60", |r| r.blacks = 60.0),
        ("blacks +100", |r| r.blacks = 100.0),
        ("shadows +76", |r| r.shadows = 76.0),
        ("shadows +100", |r| r.shadows = 100.0),
        ("contrast +100", |r| r.contrast = 100.0),
        ("old extreme combo", |r| {
            r.contrast = 100.0;
            r.highlights = -100.0;
            r.shadows = 100.0;
        }),
        ("everything at once", |r| {
            r.contrast = 100.0;
            r.highlights = 100.0;
            r.shadows = 100.0;
            r.whites = -100.0;
            r.blacks = 100.0;
        }),
        ("highlights -100", |r| r.highlights = -100.0),
        ("highlights -60", |r| r.highlights = -60.0),
        ("shadows -100", |r| r.shadows = -100.0),
        ("contrast -100", |r| r.contrast = -100.0),
        ("whites +50", |r| r.whites = 50.0),
        ("blacks -60", |r| r.blacks = -60.0),
    ];
    let interior_run = |r: &EditRecipe| -> (usize, usize, u16) {
        let lut = build_tone_lut(r);
        let out: Vec<u16> = (0..N)
            .map(|i| {
                let x = i as f32 / (N - 1) as f32;
                (sample_lut(&lut, x).clamp(0.0, 1.0) * 65535.0).round() as u16
            })
            .collect();
        let (mut run, mut worst, mut worst_at) = (1usize, 1usize, 0usize);
        for i in 1..out.len() {
            // Clipping is the user's own request; an interior plateau is
            // detail that no later stage can recover.
            run = if out[i] == out[i - 1] && out[i] > 0 && out[i] < u16::MAX {
                run + 1
            } else {
                1
            };
            if run > worst {
                worst = run;
                worst_at = i;
            }
        }
        (worst, worst_at, out[worst_at])
    };

    // ONE bound for the WHOLE grid. Until the weighted-knot model
    // (`tone_knot_weights`, M-T1) this test carried a 220-input carve-out
    // for `ev > 1.0`: the basis added slider offsets at knots whose base
    // intervals exposure had already saturated (`base_gap <= 1e-6`, the λ
    // limiter's rightful skip case), a strong negative slider dipped
    // below the ceiling, and the monotone backstop flattened a whole
    // interval at an interior grey — `contrast: -100` at `+1.5 EV`
    // flattened 197 inputs at code 56304. Four knot-LEVEL repairs were
    // measured and rejected (each traded the tail for a worse one; the
    // pre-weights per-slider λ took the worst from 197 to 317) before the
    // tone-MODEL fix landed: knot authority now follows the base curve's
    // own local separation, a slider aimed at a clipped region yields
    // honest clipping, and the measured grid is 6 cells > 96 with a
    // global worst of 100 — the same level the ev = 0 design holds, three
    // of those six sitting one code below pure white (65534, the
    // quantisation edge of clipping, not a band).
    for ev in [-3.0f32, -2.0, -1.5, -1.0, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0, 1.28, 1.5, 2.0, 3.0] {
        for (name, apply) in sliders {
            let mut r = EditRecipe { exposure_ev: ev, ..Default::default() };
            apply(&mut r);
            let (worst, at, code) = interior_run(&r);
            assert!(
                worst <= MAX_RUN,
                "{name} at {ev:+} EV: {worst} consecutive inputs (around x={:.4}) all render                      to the interior code {code} — a slider flattened a tonal band instead of                      saturating",
                at as f32 / (N - 1) as f32
            );
        }
    }
}

/// M-T2: the per-slider λ iteration removed (every `lam` left at 1, only
/// the single-λ backstop applied) — the slider that binds an interval
/// must saturate ALONE; the sliders that did not close it keep their
/// authority. This was the model's second known gap: pinning shadows +50
/// while dragging whites −100 rendered the shadows at 22.5.
#[test]
fn a_slider_that_binds_an_interval_no_longer_drags_the_innocent_ones() {
    // whites −100 alone closes the 0.92–1.0 interval and must saturate
    // near −0.45 (the value the interval can absorb); shadows was not
    // involved and keeps exactly what the caller asked.
    let out = limit_tone_sliders(0.0, [0.0, 0.0, 0.5, -1.0, 0.0]);
    assert_eq!(out[2], 0.5, "shadows were scaled for a violation they did not cause");
    assert!(
        (-0.46..=-0.44).contains(&out[3]),
        "whites did not saturate at the interval's own capacity: {}",
        out[3]
    );
    // blacks −100 OPENS the bottom interval (black point down) — no limit
    // applies to it at all, even alongside the binding whites.
    let out = limit_tone_sliders(0.0, [0.0, 0.0, 0.5, -1.0, -1.0]);
    assert_eq!(out[4], -1.0, "blacks bind nothing and must pass through untouched");
    assert_eq!(out[2], 0.5);
    // And a genuinely unconstrained vector is bit-for-bit untouched.
    let s = [0.3, -0.2, 0.4, 0.1, -0.25];
    assert_eq!(limit_tone_sliders(0.0, s), s);
}

#[test]
fn tone_lut_is_monotonic_and_keeps_midtone_separation() {
    // The reported "flat muddy water": strong opposing highlights/shadows made
    // the per-region tone curve non-monotonic and collapsed mid-bright tones
    // into one dark band. The curve must stay monotonic and keep midtones apart.
    let r = EditRecipe {
        highlights: -73.89,
        shadows: 33.28,
        whites: 6.99,
        blacks: -12.94,
        contrast: 4.68,
        ..Default::default()
    };
    let lut = build_tone_lut(&r);
    for i in 1..lut.len() {
        assert!(lut[i] >= lut[i - 1] - 1e-6, "tone LUT inverts at {i}: {} < {}", lut[i], lut[i - 1]);
    }
    // mid-bright water tones (0.50 vs 0.66) must NOT collapse to one value.
    let (a, b) = (sample_lut(&lut, 0.50), sample_lut(&lut, 0.66));
    assert!(b - a > 0.05, "midtone separation crushed flat: {a}..{b}");
    // and a true midtone (0.5) is no longer crushed deep into shadow.
    assert!(a > 0.45, "midtone water still crushed dark: {a}");
}

#[test]
fn aggressive_highlights_keep_saturated_water_from_greying() {
    // Reported bug: strong −highlights + +shadows turned bright turquoise water
    // flat grey, because the tone LUT ran per-channel and the channels converged.
    // Luminance-preserving tone must keep the cyan recognizably cyan (just darker).
    let r = EditRecipe {
        highlights: -73.89,
        shadows: 33.28,
        whites: 6.99,
        blacks: -12.94,
        contrast: 4.68,
        ..Default::default()
    };
    let cyan = [0.35_f32, 0.62, 0.66]; // mid-bright sunlit turquoise
    let mut data = vec![cyan];
    apply_develop_anon(&mut data, 1, 1, &r);
    let [rr, gg, bb] = data[0];
    // green & blue stay clearly above red → still cyan, not neutral grey.
    assert!(gg > rr + 0.08 && bb > rr + 0.08, "water greyed out: [{rr}, {gg}, {bb}]");
    // channel spread preserved (not converged toward equal = grey).
    let spread = rr.max(gg).max(bb) - rr.min(gg).min(bb);
    assert!(spread > 0.12, "channels converged toward grey: spread {spread}");
}

#[test]
fn linear_mask_affects_only_the_full_end() {
    // Linear mask: zero at top (ny=0), full at bottom (ny=1) + strong darken.
    let r = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.0, full_x: 0.5, full_y: 1.0 },
            amount: 1.0,
            exposure_ev: -4.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let (w, h) = (1usize, 4usize);
    let mut data = vec![[0.6_f32, 0.6, 0.6]; w * h];
    apply_develop_anon(&mut data, w, h, &r);
    // The four rows sample at their CENTRES, ny = (y + 0.5)/4, so the
    // weights are exactly 1/8, 3/8, 5/8, 7/8 — the top row is no longer
    // AT the gradient's zero end, it is an eighth of the way past it, and
    // it darkens according to the shipped profile (R29 C2's half-pixel
    // move, visible here). Pinned EXACTLY rather than bounded:
    // a degenerate linear (zero == full) renders weight 1 everywhere, so
    // this comparison carries the identical shipped weight through every
    // stage below it.
    let top_weight = linear_coverage(0.125, LINEAR_FALLOFF);
    let eighth = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.5, full_x: 0.5, full_y: 0.5 },
            amount: top_weight,
            exposure_ev: -4.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut eighth_px = vec![[0.6_f32, 0.6, 0.6]; 1];
    apply_develop_anon(&mut eighth_px, 1, 1, &eighth);
    assert_eq!(data[0], eighth_px[0], "top row carries exactly its shipped coverage");
    assert!(data[0][0] < 0.6, "…which is a real darkening: {}", data[0][0]);
    assert!(data[3][0] < 0.5, "bottom should darken: {}", data[3][0]);
    // The interior rows carry the actual gradient — the endpoint checks
    // alone let a "positive weight ⇒ full coverage" mutation render the
    // ramp as a hard step (row 0 has weight EXACTLY 0 and stays pinned;
    // row 3 only darkens further) (U14).
    assert!(
        data[1][0] > data[2][0] + 0.05 && data[2][0] > data[3][0] + 0.05,
        "linear ramp collapsed to a step: {:?}",
        [data[1][0], data[2][0], data[3][0]]
    );
}

#[test]
fn local_noise_reduction_smooths_only_inside_the_mask() {
    // 8x1 strip of alternating luma (= noise). A linear mask covering the
    // RIGHT half with full local NR should flatten the right; left untouched.
    //
    // ±0.03, a real noise amplitude. Since v1.5.0 the local pass is the
    // global Detail operator (`render/detail.rs`), which is EDGE-AWARE: a
    // ±0.2 alternation is structure to it at any amount (local σ 0.2
    // against a noise threshold of 0.054 at Luminance 100), where the
    // pre-v1.5.0 local pass was a plain blur that flattened anything.
    let (w, h) = (8usize, 1usize);
    let mut data: Vec<[f32; 3]> =
        (0..w).map(|x| { let v = if x % 2 == 0 { 0.47 } else { 0.53 }; [v, v, v] }).collect();
    let r = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.5, full_x: 1.0, full_y: 0.5 },
            amount: 1.0,
            noise_reduction: 100.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let var = |d: &[[f32; 3]], rng: std::ops::Range<usize>| {
        let v: Vec<f32> = rng.map(|i| d[i][0]).collect();
        let m = v.iter().sum::<f32>() / v.len() as f32;
        v.iter().map(|x| (x - m).powi(2)).sum::<f32>() / v.len() as f32
    };
    // Control render (mask-less, same global stages): "untouched" must
    // mean BIT-FOR-BIT equal — the convention the range-mask tests use.
    // The old probe compared the left half's red-channel VARIANCE, which
    // is blind to a constant offset (translation-invariant) and to
    // green/blue-only leaks (it never read those channels) (U14).
    let mut control = data.clone();
    apply_develop_anon(&mut control, w, h, &EditRecipe::default());
    let right0 = var(&data, 4..8);
    apply_develop_anon(&mut data, w, h, &r);
    assert!(var(&data, 4..8) < right0 * 0.8, "right half should smooth");
    assert_eq!(&data[0..4], &control[0..4], "left half untouched, bit for bit");
}

#[test]
fn luminance_range_mask_gates_by_pixel_brightness() {
    // Full-coverage geometry (degenerate linear = weight 1 everywhere) so
    // ONLY the luminance range decides where the −2 EV darken lands. The
    // trapezoid uses a degenerate top edge (hi == hi_outer == 1.0), exactly
    // like the real ACR sidecars' `LumRange="… 1.000000 1.000000"`.
    let full = MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.5, full_x: 0.5, full_y: 0.5 };
    let r = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: full,
            range: Some(RangeMask::Luminance { lo_outer: 0.55, lo: 0.7, hi: 1.0, hi_outer: 1.0 }),
            amount: 1.0,
            exposure_ev: -2.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let dark = [0.15_f32; 3];
    let mid = [0.625_f32; 3]; // ramp midpoint between lo_outer and lo
    let bright = [0.85_f32; 3];
    // Control: identical pipeline WITHOUT the mask. The global stages run
    // either way (the neutral tone LUT still costs ~1 ULP of interpolation
    // noise), so "untouched by the mask" means equal to the CONTROL, not to
    // the raw input.
    let mut control = vec![dark, mid, bright];
    apply_develop_anon(&mut control, 3, 1, &EditRecipe::default());
    let mut data = vec![dark, mid, bright];
    apply_develop_anon(&mut data, 3, 1, &r);
    assert_eq!(data[0], control[0], "below the range: the mask must skip it");
    assert!(data[2][0] < 0.6, "bright pixel must darken: {}", data[2][0]);
    // The ramp midpoint moves, but less than the fully-selected pixel.
    let (d_mid, d_bright) = (control[1][0] - data[1][0], control[2][0] - data[2][0]);
    assert!(d_mid > 0.01 && d_mid < d_bright, "feathered ramp: mid {d_mid} vs bright {d_bright}");
}

#[test]
fn color_range_mask_selects_chroma_not_brightness() {
    // Desaturate through a colour range keyed to orange: both bright and
    // dark orange collapse to grey (luminance-invariant match), while blue
    // and neutral grey pass through bit-exact.
    let full = MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.5, full_x: 0.5, full_y: 0.5 };
    let r = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: full,
            range: Some(RangeMask::Color { r: 0.9, g: 0.6, b: 0.2, amount: 0.5, px: 0.5, py: 0.5 }),
            amount: 1.0,
            saturation: -100.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let orange = [0.9_f32, 0.6, 0.2];
    let dark_orange = [0.45_f32, 0.3, 0.1]; // same chromaticity, half as bright
    let blue = [0.2_f32, 0.3, 0.9];
    let grey = [0.6_f32; 3];
    // Same control-render comparison as the luminance test: out-of-range
    // pixels must match a mask-less render exactly (the mask pass skips them).
    let mut control = vec![orange, dark_orange, blue, grey];
    apply_develop_anon(&mut control, 4, 1, &EditRecipe::default());
    let mut data = vec![orange, dark_orange, blue, grey];
    apply_develop_anon(&mut data, 4, 1, &r);
    let spread = |p: [f32; 3]| p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2]);
    assert!(spread(data[0]) < 0.05, "orange must desaturate: {:?}", data[0]);
    assert!(spread(data[1]) < 0.05, "dark orange (same hue) must desaturate: {:?}", data[1]);
    assert_eq!(data[2], control[2], "opposite hue: the mask must skip it");
    assert_eq!(data[3], control[3], "neutral grey: the mask must skip it");
    // Desaturation must land at the pixel's LUMA, not at black — spread
    // alone cannot tell grey from destroyed (a `c * factor` rewrite of
    // the sat formula yields [0,0,0] with spread 0 and passed) (U14).
    let lum = |p: [f32; 3]| 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2];
    assert!(
        (data[0][0] - lum(control[0])).abs() < 0.03,
        "orange must keep its brightness: {:?} vs luma {}",
        data[0],
        lum(control[0])
    );
    assert!(
        (data[1][0] - lum(control[1])).abs() < 0.03,
        "dark orange must keep its brightness: {:?} vs luma {}",
        data[1],
        lum(control[1])
    );
}

#[test]
fn local_temperature_warms_the_masked_region_only() {
    // Feedback batch #2-B prerequisite: LocalAdjustment carried Temp/Tint
    // since v1 and the XMP writer exports them, but the ENGINE ignored
    // them (render.rs listed them as "deferred") — so the GUI's mask
    // Temp/Tint sliders did nothing in-app, and the zoned reverse-fit
    // would have nothing to drive. A warm local temperature must boost
    // red / cut blue inside the mask; a row of weight EXACTLY 0 must stay
    // equal to a mask-less control render (the mask pass skips it).
    //
    // The gradient's zero end sits at `zero_y = 0.125`, which is the TOP
    // ROW'S CENTRE on this 4-row frame (R29 C2: rows sample at
    // ny = (y + 0.5)/4). It has to, for the skip to be exercised at all —
    // under pixel-centre sampling no row of a 0→1 gradient carries weight
    // 0, and the old `zero_y = 0.0` fixture stopped testing the skip the
    // moment the convention was corrected. The weights here are exactly
    // 0, 1/3, 2/3, 1: `(ny − 0.125)·0.75 / 0.5625`, all dyadic.
    let r = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Linear {
                zero_x: 0.5, zero_y: 0.125, full_x: 0.5, full_y: 0.875,
            },
            amount: 1.0,
            temperature: 100.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let grey = [0.5_f32; 3];
    let (w, h) = (1usize, 4usize);
    let mut control = vec![grey; w * h];
    apply_develop_anon(&mut control, w, h, &EditRecipe::default());
    let mut data = vec![grey; w * h];
    apply_develop_anon(&mut data, w, h, &r);
    assert_eq!(data[0], control[0], "zero end of the gradient: the mask must skip it");
    let px = data[3];
    assert!(
        px[0] > grey[0] + 0.02 && px[2] < grey[2] - 0.02,
        "full end must warm (red up, blue down): {px:?}"
    );
    // …and the SAME geometry read on the old `y/h` grid would give the top
    // row weight (0 − 0.125)·0.75/0.5625 < 0 → clamped to 0 as well, so the
    // skip alone cannot separate the two conventions. The bottom row can:
    // at ny = 0.875 the weight is exactly 1, while `y/h` puts row 3 at
    // ny = 0.75 → weight 5/6, short of the full end. Assert the full end is
    // REACHED by comparing against an amount-1 whole-frame mask.
    let full = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.5, full_x: 0.5, full_y: 0.5 },
            amount: 1.0,
            temperature: 100.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut full_px = vec![grey; 1];
    apply_develop_anon(&mut full_px, 1, 1, &full);
    assert_eq!(data[3], full_px[0], "the last row's centre IS the gradient's full end");
}

#[test]
fn colour_gain_lut_matches_the_exact_linear_light_formula() {
    // Pin the optimization's fidelity independently of apply_wb: compare
    // LUT interpolation against the old exact formula over dark values
    // (where the sRGB knee is hardest), midtones and highlights, across
    // both sub-unity and strong zoned-fit gains. The tolerance is below
    // one 16-bit code value (1/65535 ≈ 1.53e-5).
    for gains in [[0.41f32, 0.91, 1.45], [1.65, 0.76, 0.38], [1.0, 1.0, 1.0]] {
        let luts = colour_gain_luts(gains);
        for x in [0.0f32, 0.001, 0.003, 0.01, 0.04, 0.1, 0.25, 0.5, 0.8, 0.99, 1.0] {
            for ch in 0..3 {
                let exact =
                    linear_to_srgb((srgb_to_linear(x) * gains[ch]).clamp(0.0, 1.0));
                let fast = sample_lut(&luts[ch], x);
                assert!(
                    (fast - exact).abs() < 1.5e-5,
                    "gain {} x {x}: LUT {fast} vs exact {exact}",
                    gains[ch]
                );
            }
        }
    }
}

#[test]
fn full_frame_local_wb_matches_the_global_wb_stage() {
    // The local Temp/Tint must MIRROR apply_recipe_wb's semantics — same
    // wb_gains model, same 5500 K anchor, WB→tone→sat order — so a
    // weight-1 full-frame mask must land within LUT-quantization of a
    // global render whose absolute Kelvin is the mired-mapped target.
    // This pins local_temp_to_kelvin AND the tint sign end to end.
    let full = MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.5, full_x: 0.5, full_y: 0.5 };
    let (t_rel, tint) = (60.0_f32, -25.0_f32);
    let src = [[0.7_f32, 0.55, 0.35], [0.2, 0.35, 0.6], [0.5, 0.5, 0.5]];
    let mut global = src.to_vec();
    apply_recipe_wb(
        &mut global,
        &EditRecipe {
            temperature_k: Some(local_temp_to_kelvin(t_rel)),
            tint,
            ..Default::default()
        },
    );
    apply_develop_anon(&mut global, 3, 1, &EditRecipe::default());
    let mut local = src.to_vec();
    apply_develop_anon(
        &mut local,
        3,
        1,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                mask: full,
                amount: 1.0,
                temperature: t_rel,
                tint,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    for (a, b) in global.iter().zip(&local) {
        for c in 0..3 {
            assert!(
                (a[c] - b[c]).abs() < 3e-3,
                "full-frame local WB drifted from the global stage: {a:?} vs {b:?}"
            );
        }
    }
}

#[test]
fn exports_are_tagged_srgb_in_all_three_formats() {
    // Every export format must carry the sRGB profile: JPEG in an APP2
    // "ICC_PROFILE" segment, PNG in an iCCP chunk, TIFF as the raw profile
    // (tag 34675) whose header signature is "acsp".
    std::fs::create_dir_all("out").ok();
    let src_p = std::path::Path::new("out/_icc_src.png");
    RgbImage::from_fn(32, 16, |x, y| Rgb([(x * 8) as u8, (y * 16) as u8, 128]))
        .save(src_p)
        .unwrap();
    let neutral = EditRecipe::default();
    for (name, needle) in [
        ("out/_icc.jpg", &b"ICC_PROFILE"[..]),
        ("out/_icc.png", &b"iCCP"[..]),
        ("out/_icc.tif", &b"acsp"[..]),
    ] {
        render_to_file(src_p, &neutral, std::path::Path::new(name), None, None, crate::diag::stderr()).unwrap();
        let bytes = std::fs::read(name).unwrap();
        assert!(
            bytes.windows(needle.len()).any(|win| win == needle),
            "{name} must carry the sRGB ICC marker"
        );
        // The markers above match ANY ICC payload — pin the PROFILE:
        // tag_icc's Srgb arm shipping the P3/Adobe bytes passed every
        // marker check (U14). JPEG/TIFF store the profile verbatim; PNG
        // deflate-compresses it, so compare the DECODED profile.
        if name.ends_with(".png") {
            let mut d = image::codecs::png::PngDecoder::new(std::io::BufReader::new(
                std::fs::File::open(name).unwrap(),
            ))
            .unwrap();
            assert_eq!(
                image::ImageDecoder::icc_profile(&mut d).unwrap().unwrap(),
                SRGB_ICC.to_vec(),
                "{name} must embed the sRGB profile (decompressed iCCP)"
            );
        } else {
            assert!(
                bytes.windows(SRGB_ICC.len()).any(|win| win == SRGB_ICC),
                "{name} must embed the sRGB profile bytes"
            );
        }
    }
}

#[test]
fn gamut_transform_is_colorimetric_not_a_tag_swap() {
    // (a) White preservation pins the whole matrix derivation: every row of
    // sRGB→target must sum to 1 (R=G=B=1 stays exactly white — all three
    // spaces share the D65 white point, so no adaptation term may appear).
    for space in [ExportColorSpace::DisplayP3, ExportColorSpace::AdobeRgb] {
        let m = srgb_to_space_matrix(space).unwrap();
        for (i, row) in m.iter().enumerate() {
            let s: f32 = row.iter().sum();
            assert!((s - 1.0).abs() < 1e-3, "{space:?} row {i} sums to {s}");
        }
        // (b) Invertibility: a color grid survives forward → inverse.
        let inv = inv3(&m);
        for c in [[1.0f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.7, 0.2, 0.55]] {
            let back = mat_vec3(&inv, &mat_vec3(&m, &c));
            for k in 0..3 {
                assert!((back[k] - c[k]).abs() < 1e-3, "{space:?} roundtrip {c:?} → {back:?}");
            }
        }
    }

    // (c) Full pixel path on a mid-grey: P3 shares sRGB's TRC, so a neutral
    // pixel is numerically UNCHANGED; Adobe RGB's pure gamma encodes the
    // same grey darker — while staying exactly neutral. That difference is
    // the transform actually running (a tag swap would leave both equal).
    let grey = DynamicImage::ImageRgb16(ImageBuffer::from_pixel(2, 2, Rgb([32896u16, 32896, 32896])));
    let p3 = convert_export_color_space(grey.clone(), ExportColorSpace::DisplayP3).to_rgb16();
    let (pr, pg, pb) = (p3.get_pixel(0, 0)[0], p3.get_pixel(0, 0)[1], p3.get_pixel(0, 0)[2]);
    assert!(pr == pg && pg == pb, "P3 grey must stay neutral: {pr},{pg},{pb}");
    assert!((pr as i32 - 32896).abs() <= 4, "P3 grey must keep its value: {pr}");
    let ad = convert_export_color_space(grey.clone(), ExportColorSpace::AdobeRgb).to_rgb16();
    let (ar, ag, ab) = (ad.get_pixel(0, 0)[0], ad.get_pixel(0, 0)[1], ad.get_pixel(0, 0)[2]);
    assert!(ar == ag && ag == ab, "AdobeRGB grey must stay neutral: {ar},{ag},{ab}");
    assert!((ar as i32) < pr as i32 - 64, "AdobeRGB gamma must encode grey darker: {ar} vs {pr}");

    // (d) Saturated sRGB red. P3's red primary sits further out, so sRGB
    // red lands strictly INSIDE (dominant red, positive green/blue).
    // Adobe RGB shares sRGB's red CHROMATICITY, so sRGB red stays a pure
    // red there — just rescaled (Adobe's red carries a larger luminance
    // share): g = b = 0 with red below full scale. Both derive from the
    // primaries table, so both directions pin the matrix.
    let red = DynamicImage::ImageRgb16(ImageBuffer::from_pixel(1, 1, Rgb([65535u16, 0, 0])));
    let p3r = convert_export_color_space(red.clone(), ExportColorSpace::DisplayP3).to_rgb16();
    let p = p3r.get_pixel(0, 0);
    assert!(
        p[0] > 55000 && p[1] > 0 && p[2] > 0 && p[1] < p[0] && p[2] < p[0],
        "DisplayP3: sRGB red must land inside the gamut, got {p:?}"
    );
    let adr = convert_export_color_space(red, ExportColorSpace::AdobeRgb).to_rgb16();
    let q = adr.get_pixel(0, 0);
    assert!(
        q[0] > 50000 && q[0] < 62000 && q[1] <= 300 && q[2] <= 300,
        "AdobeRGB: sRGB red must stay a rescaled pure red, got {q:?}"
    );

    // (d2) Green and blue primaries pin the REMAINING columns: the red
    // probe alone survives a green/blue column swap of the matrix — row
    // sums, invertibility, grey and red are all invariant under it, and
    // so is the calibration cross-check, which derives from the same
    // primaries table (U14). All three spaces share the same blue
    // CHROMATICITY, so sRGB blue stays a pure blue in both targets;
    // under the swap each primary would receive the other's column.
    let green = DynamicImage::ImageRgb16(ImageBuffer::from_pixel(1, 1, Rgb([0u16, 65535, 0])));
    let blue = DynamicImage::ImageRgb16(ImageBuffer::from_pixel(1, 1, Rgb([0u16, 0, 65535])));
    for space in [ExportColorSpace::DisplayP3, ExportColorSpace::AdobeRgb] {
        let g = convert_export_color_space(green.clone(), space).to_rgb16();
        let p = g.get_pixel(0, 0);
        assert!(
            p[1] > 50000 && p[0] < p[1] && p[2] < p[1],
            "{space:?}: sRGB green must stay dominant-green, got {p:?}"
        );
        let b = convert_export_color_space(blue.clone(), space).to_rgb16();
        let p = b.get_pixel(0, 0);
        assert!(
            p[2] > 55000 && p[0] < 1000 && p[1] < 1000,
            "{space:?}: sRGB blue shares the target's blue primary — must stay pure, got {p:?}"
        );
    }

    // (e) sRGB is the identity (now a MOVE, not a clone).
    let same = convert_export_color_space(grey, ExportColorSpace::Srgb).to_rgb16();
    assert_eq!(same.get_pixel(1, 1)[0], 32896);
}

#[test]
fn wide_develop_calibration_agrees_with_the_export_matrix() {
    // A synthetic camera whose native space IS sRGB: xyz2cam =
    // inv(sRGB→XYZ). The DNG calibration into a target space must then
    // equal the sRGB→target export matrix — the two derivations meet.
    let xyz2cam = inv3(&rgb_to_xyz(SRGB_PRIM, D65_XY));
    for space in [ExportColorSpace::DisplayP3, ExportColorSpace::AdobeRgb] {
        let cam2space = camera_to_space_matrix(&xyz2cam, space);
        let reference = srgb_to_space_matrix(space).unwrap();
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (cam2space[i][j] - reference[i][j]).abs() < 2e-3,
                    "{space:?} [{i}][{j}]: {} vs {}",
                    cam2space[i][j],
                    reference[i][j]
                );
            }
        }
    }
}

#[test]
fn wide_develop_keeps_out_of_srgb_camera_colors_and_neutral_parity() {
    // A camera whose native space IS Display P3: its pure red lies
    // OUTSIDE sRGB. Developed INTO DisplayP3 it must survive as ~[1,0,0]
    // — the colour the old clip-at-sRGB pipeline destroyed.
    let xyz2cam = inv3(&rgb_to_xyz(P3_PRIM, D65_XY));
    let mut px = [[1.0f32, 0.0, 0.0]];
    calibrate_camera_buffer(&mut px, &xyz2cam, [1.0, 1.0, 1.0], ExportColorSpace::DisplayP3, None);
    let p = px[0];
    assert!(
        p[0] > 0.99 && p[1].abs() < 0.02 && p[2].abs() < 0.02,
        "P3-native red must survive a P3 develop, got {p:?}"
    );
    // The same HUE at 80% (an unblown saturated colour — full-scale 1.0
    // is a clipped sensor reading and legitimately takes the highlight
    // desaturation, same as rawler) into sRGB goes out of gamut: a
    // NEGATIVE component must reach the caller (the final pack clips it
    // — not the decode).
    let mut px = [[0.8f32, 0.0, 0.0]];
    calibrate_camera_buffer(&mut px, &xyz2cam, [1.0, 1.0, 1.0], ExportColorSpace::Srgb, None);
    assert!(
        px[0][1] < -0.001 || px[0][2] < -0.001,
        "out-of-gamut components must SURVIVE to the pack, got {:?}",
        px[0]
    );
    // Neutral parity: a white-balanced grey encodes to the SAME value in
    // every space (shared D65 white + shared working transfer) — the
    // whole reason the wide develop may share the sRGB tone pipeline.
    let wb = [2.0f32, 1.0, 1.5];
    let grey_cam = [[0.4 / 2.0, 0.4, 0.4 / 1.5]];
    let mut out = [[0.0f32; 3]; 3];
    for (i, space) in
        [ExportColorSpace::Srgb, ExportColorSpace::DisplayP3, ExportColorSpace::AdobeRgb]
            .into_iter()
            .enumerate()
    {
        let mut px = grey_cam;
        calibrate_camera_buffer(&mut px, &xyz2cam, wb, space, None);
        out[i] = px[0];
    }
    let want = linear_to_srgb(0.4);
    for (i, o) in out.iter().enumerate() {
        for c in o {
            assert!((c - want).abs() < 2e-3, "space {i}: grey drifted to {o:?} (want {want})");
        }
    }
}

#[test]
fn adobe_trc_transcode_matches_the_conversion_path_on_neutrals() {
    // A grey through the native-wide path (primaries already Adobe,
    // transfer swap only) must land where the matrix path lands it —
    // on neutrals the matrix is a no-op, isolating the TRC.
    let grey = DynamicImage::ImageRgb16(ImageBuffer::from_pixel(1, 1, Rgb([32896u16, 32896, 32896])));
    let via_matrix =
        convert_export_color_space(grey.clone(), ExportColorSpace::AdobeRgb).to_rgb16();
    let via_transcode = transcode_srgb_trc_to_adobe(grey).to_rgb16();
    let (a, b) = (via_matrix.get_pixel(0, 0), via_transcode.get_pixel(0, 0));
    for c in 0..3 {
        assert!(
            (a[c] as i32 - b[c] as i32).abs() <= 1,
            "TRC transcode must agree with the conversion path: {a:?} vs {b:?}"
        );
    }
}

#[test]
fn exports_embed_the_selected_wide_gamut_profile() {
    // JPEG (APP2, one segment at 736 B) and TIFF (tag 34675) store the raw
    // profile — the ENTIRE profile bytes must appear in the file. PNG
    // deflate-compresses inside iCCP, so its profile is compared
    // DECOMPRESSED via the decoder — the old claim that the sRGB test's
    // chunk check covered it was FALSE: that check only proves an iCCP
    // chunk exists, so a PNG arm shipping sRGB bytes under a P3
    // selection passed everything (U14).
    std::fs::create_dir_all("out").ok();
    let src_p = std::path::Path::new("out/_gamut_src.png");
    RgbImage::from_fn(24, 12, |x, y| Rgb([(x * 10) as u8, (y * 20) as u8, 90]))
        .save(src_p)
        .unwrap();
    let neutral = EditRecipe::default();
    for (space, profile) in [
        (ExportColorSpace::DisplayP3, DISPLAY_P3_ICC),
        (ExportColorSpace::AdobeRgb, ADOBE_RGB_ICC),
    ] {
        let opts = ExportOpts { color_space: space, ..Default::default() };
        for name in ["out/_gamut.jpg", "out/_gamut.tif"] {
            render_to_file(src_p, &neutral, std::path::Path::new(name), None, Some(&opts), crate::diag::stderr()).unwrap();
            let bytes = std::fs::read(name).unwrap();
            assert!(
                bytes.windows(profile.len()).any(|win| win == profile),
                "{name} must embed the full {space:?} profile ({} B)",
                profile.len()
            );
        }
        let png_name = "out/_gamut.png";
        render_to_file(src_p, &neutral, std::path::Path::new(png_name), None, Some(&opts), crate::diag::stderr()).unwrap();
        let mut d = image::codecs::png::PngDecoder::new(std::io::BufReader::new(
            std::fs::File::open(png_name).unwrap(),
        ))
        .unwrap();
        assert_eq!(
            image::ImageDecoder::icc_profile(&mut d).unwrap().unwrap(),
            profile.to_vec(),
            "{png_name} must embed the full {space:?} profile"
        );
    }
}

#[test]
fn vignette_gain_is_radial_and_linear_light() {
    // A flat mid-grey field: +60 compensation must leave the exact centre
    // untouched, brighten the corner the most, and increase monotonically
    // with radius. Negative amount darkens the corner instead.
    let (w, h) = (9usize, 9usize);
    let flat = vec![[0.5_f32; 3]; w * h];
    let mut up = flat.clone();
    apply_radial_gain(&mut up, w, h, &manual_vignette_lut(60.0, 50.0));
    let centre = up[4 * w + 4][0];
    let mid = up[2 * w + 2][0]; // halfway toward the corner
    let corner = up[0][0];
    assert!((centre - 0.5).abs() < 1e-4, "centre must not move: {centre}");
    assert!(corner > mid && mid > centre, "radial monotone: {centre} < {mid} < {corner}");
    assert!(corner > 0.62, "corner must clearly brighten: {corner}");
    // Pin the NAMED linear-light formula at the exact corner (rn = 1 →
    // gain = 1.6): decode → gain → encode. Gamma-space multiplication
    // (0.5·1.6 = 0.8) clears every shape assertion here yet misses this
    // by 0.18 (U14).
    let expect = linear_to_srgb((srgb_to_linear(0.5) * 1.6).min(1.0));
    assert!(
        (corner - expect).abs() < 2e-3,
        "corner must follow the linear-light gain: {corner} vs {expect}"
    );
    // Grey in, grey out: the gain applies to all three channels — every
    // assertion above reads channel 0 only, so a red-only regression of
    // the per-channel loop passed with a strong colour cast (R12).
    for p in [&up[0], &up[2 * w + 2]] {
        assert!(
            (p[0] - p[1]).abs() < 1e-6 && (p[1] - p[2]).abs() < 1e-6,
            "vignette must stay neutral on grey: {p:?}"
        );
    }

    let mut down = flat.clone();
    apply_radial_gain(&mut down, w, h, &manual_vignette_lut(-60.0, 50.0));
    assert!(down[0][0] < 0.38, "negative amount darkens the corner: {}", down[0][0]);

    // Higher midpoint confines the effect to the corners: the halfway
    // pixel moves LESS than with the default midpoint.
    let mut tight = flat.clone();
    apply_radial_gain(&mut tight, w, h, &manual_vignette_lut(60.0, 100.0));
    assert!(tight[2 * w + 2][0] < mid, "midpoint 100 must spare the mid-field");
}

#[test]
fn export_opts_resize_sharpen_quality() {
    // Synthetic 200×100 gradient source (baked path), rendered through the
    // delivery pipeline. Long edge 50 → 50×25 saved AND reported; a long
    // edge larger than the source never upscales; lower JPEG quality
    // produces a smaller file than higher quality.
    std::fs::create_dir_all("out").ok();
    let src_p = std::path::Path::new("out/_export_src.png");
    let img = RgbImage::from_fn(200, 100, |x, y| {
        Rgb([(x % 256) as u8, (y * 2 % 256) as u8, ((x + y) % 256) as u8])
    });
    img.save(src_p).unwrap();
    let neutral = EditRecipe::default();

    let small = ExportOpts { long_edge: Some(50), sharpen: 25.0, ..Default::default() };
    let (w, h) =
        render_to_file(src_p, &neutral, std::path::Path::new("out/_export_le50.png"), None, Some(&small), crate::diag::stderr())
            .unwrap();
    assert_eq!((w, h), (50, 25), "long edge 50 must fit 200×100 to 50×25");
    let saved = image::image_dimensions("out/_export_le50.png").unwrap();
    assert_eq!(saved, (50, 25), "saved file dims must match the report");

    let big = ExportOpts { long_edge: Some(400), ..Default::default() };
    let (w, h) =
        render_to_file(src_p, &neutral, std::path::Path::new("out/_export_le400.png"), None, Some(&big), crate::diag::stderr())
            .unwrap();
    assert_eq!((w, h), (200, 100), "long edge beyond source must NOT upscale");

    for (q, name) in [(30u8, "out/_export_q30.jpg"), (95u8, "out/_export_q95.jpg")] {
        let opts = ExportOpts { jpeg_quality: q, ..Default::default() };
        render_to_file(src_p, &neutral, std::path::Path::new(name), None, Some(&opts), crate::diag::stderr()).unwrap();
    }
    let (s30, s95) = (
        std::fs::metadata("out/_export_q30.jpg").unwrap().len(),
        std::fs::metadata("out/_export_q95.jpg").unwrap().len(),
    );
    assert!(s30 < s95, "q30 ({s30} B) must be smaller than q95 ({s95} B)");

    // Output sharpening must be OBSERVABLE: same source, same size, only
    // `sharpen` differs — the sharpened file needs strictly more edge
    // energy. Deleting the whole post-resize stage changed no previously
    // asserted quantity (dims and JPEG sizes are blind to it) (U14).
    let edge_p = std::path::Path::new("out/_export_edge.png");
    RgbImage::from_fn(200, 100, |x, _| Rgb([if x < 100 { 40 } else { 215 }; 3]))
        .save(edge_p)
        .unwrap();
    let export = |sharpen: f32, name: &str| {
        let opts = ExportOpts { long_edge: Some(100), sharpen, ..Default::default() };
        render_to_file(edge_p, &neutral, std::path::Path::new(name), None, Some(&opts), crate::diag::stderr()).unwrap();
        let px = image::open(name).unwrap().to_rgb8();
        let (w, h) = px.dimensions();
        // Per channel: a red-only sharpen raised the summed energy too,
        // so each channel gets its own comparison (R12).
        let mut energy = [0u64; 3];
        for y in 0..h {
            for x in 1..w {
                for (c, e) in energy.iter_mut().enumerate() {
                    *e += (px[(x, y)][c] as i64 - px[(x - 1, y)][c] as i64).unsigned_abs();
                }
            }
        }
        ((w, h), energy)
    };
    let (dim_flat, e_flat) = export(0.0, "out/_export_sharp0.png");
    let (dim_sharp, e_sharp) = export(100.0, "out/_export_sharp100.png");
    assert_eq!(dim_flat, dim_sharp, "only the sharpen knob differs");
    for c in 0..3 {
        assert!(
            e_sharp[c] > e_flat[c],
            "output sharpening must raise edge energy on channel {c}: {e_sharp:?} vs {e_flat:?}"
        );
    }
}

/// R29 Batch-2 acceptance ①: WHAT `--long-edge` is, stated as an equality.
///
/// The CLI's new `--long-edge N` reaches [`ExportOpts::long_edge`], which
/// `render_to_file` applies as its LAST pixel stage — after the whole
/// develop, after output sharpening's own input is chosen, before the
/// encode. So the delivered file is exactly the full-resolution render
/// resampled, and the equality below says so with no tolerance at all:
/// `long_edge = k` is Lanczos3 downscale of the k-less render, to the
/// 16-bit code, at every k.
///
/// That is a REAL claim and not a tautology, because three plausible
/// implementations break it and one of them was on the table:
///
/// * resampling with a different kernel (`serve`'s preview arm uses
///   `Triangle`, `src/serve.rs:860`, while `decode::preview_resized` uses
///   `Lanczos3`, `src/decode.rs:2146` — the two spellings already in this
///   tree). The CLI flag inherits the EXPORT path's `Lanczos3`
///   (`src/render.rs:1564`) because it is that path; no third choice was
///   added.
/// * developing at the capped resolution instead of capping the developed
///   pixels — see acceptance ② below for how far apart those two are.
/// * doing the resize before output sharpening's measurement, or after the
///   colour-space transform.
///
/// MEASURED, on the 800×600 fixture below with sharpening 60 / clarity 40 /
/// texture 50: mean |Δ| = 0.000000 and worst |Δ| = 0 sixteen-bit codes at
/// both k = 400 and k = 200. Pinned at 0, since anything else means one of
/// the three above happened.
#[test]
fn export_at_size_is_exactly_a_downscale_of_the_full_render() {
    std::fs::create_dir_all("out").ok();
    let src_p = std::path::Path::new("out/_le_src.png");
    // Detail at all three scales the normalised operators work on: a
    // deterministic hash for pixel-scale grain (texture/sharpening), the
    // sinusoids for mid-band structure, the ramp for the tonal range
    // clarity's midtone mask needs. A flat fixture would satisfy this test
    // with every stage deleted.
    RgbImage::from_fn(800, 600, |x, y| {
        let hash = x.wrapping_mul(2_654_435_761u32).wrapping_add(y.wrapping_mul(40_503)) >> 13;
        let grain = (hash & 31) as f32 - 15.5;
        let mid = 40.0 * ((x as f32 / 9.0).sin() + (y as f32 / 7.0).cos());
        let ramp = 90.0 + 100.0 * x as f32 / 800.0;
        let v = |off: f32| (ramp + mid + grain + off).clamp(0.0, 255.0) as u8;
        Rgb([v(0.0), v(-8.0), v(12.0)])
    })
    .save(src_p)
    .unwrap();
    let recipe =
        EditRecipe { sharpening: 60.0, clarity: 40.0, texture: 50.0, ..Default::default() };

    let full_p = std::path::Path::new("out/_le_full.png");
    let (fw, fh) =
        render_to_file(src_p, &recipe, full_p, None, None, crate::diag::stderr()).unwrap();
    assert_eq!((fw, fh), (800, 600), "no export opts = the source's own resolution");
    // 16-bit PNG, so the round trip through the file is lossless and the
    // comparison below measures the RESIZE and nothing else.
    let full = image::open(full_p).unwrap();

    for k in [400u32, 200] {
        let opts = ExportOpts { long_edge: Some(k), ..Default::default() };
        let p = format!("out/_le_{k}.png");
        let (w, h) = render_to_file(
            src_p,
            &recipe,
            std::path::Path::new(&p),
            None,
            Some(&opts),
            crate::diag::stderr(),
        )
        .unwrap();
        assert_eq!(w.max(h), k, "the LONG edge is what the flag bounds");
        assert_eq!((w, h), (k, k * 3 / 4), "…and the aspect ratio is kept");
        let got = image::open(&p).unwrap().to_rgb16();
        let want = full.resize(k, k, image::imageops::FilterType::Lanczos3).to_rgb16();
        assert_eq!(got.dimensions(), want.dimensions());
        let worst = got
            .as_raw()
            .iter()
            .zip(want.as_raw())
            .map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs())
            .max()
            .unwrap();
        assert_eq!(
            worst, 0,
            "long_edge {k} must BE the Lanczos3 downscale of the full render — \
             worst channel difference {worst} sixteen-bit codes"
        );
    }

    // `Some(0)` is FULL RESOLUTION, not "resize to nothing": the guard is
    // `le > 0` (this file, the `opts.long_edge` block), and the CLI folds
    // its own `--long-edge 0` to `None` on top of that so the two surfaces
    // cannot disagree. Pinned here because a `saturating`-flavoured
    // rewrite of that guard would produce a 1×1 deliverable in silence.
    let zero = ExportOpts { long_edge: Some(0), ..Default::default() };
    let z_p = std::path::Path::new("out/_le_zero.png");
    let (zw, zh) =
        render_to_file(src_p, &recipe, z_p, None, Some(&zero), crate::diag::stderr()).unwrap();
    assert_eq!((zw, zh), (800, 600), "long_edge 0 = full resolution");
    assert_eq!(
        image::open(z_p).unwrap().to_rgb16().as_raw(),
        full.to_rgb16().as_raw(),
        "long_edge 0 must not touch a single pixel"
    );
}

/// R29 Batch-2 acceptance ②: the R25 B2 RESOLUTION-NORMALISATION promise,
/// measured — and the boundary of what acceptance ① above buys.
///
/// The promise (`apply_develop` stages 3/3b/5, and the `texture_pass` doc)
/// is that clarity, texture and sharpening have radii expressed as a
/// FRACTION of the frame, so one slider value means the same structure on a
/// 1280 px preview and on a 61 MP export. Nothing in the tree measured it;
/// the promise lived in four comments.
///
/// This measures it where it is checkable: clarity's radius is 2 % of the
/// short edge (`src/render.rs:1833`), and a three-pass box blur of radius r
/// reaches 3r, so a step edge's halo must be 3 × 0.02 × short-edge px wide —
/// i.e. the SAME fraction of the frame at both resolutions. Doubling the
/// working resolution must double the halo in pixels.
///
/// MUTATION THIS KILLS: replacing the radius with any constant (the shape
/// the code had before R25 B2 — `unsharp_luma(data, w, h, 8, …)`), which
/// makes the ratio 1.0 instead of 2.0 while every other clarity test in
/// this file still passes, because they all work at ONE resolution.
///
/// **What this does NOT say, and it matters for `--long-edge`.** Acceptance
/// ① shows the export resize happens AFTER the develop, so `--long-edge`
/// never exercises this promise at all: the develop runs at full sensor
/// resolution and the resampler then averages its halos down with
/// everything else. Developing at the capped resolution instead is a
/// visibly different picture, and it is worth knowing by how much.
///
/// The five figures below are PROVENANCED but not gate-checked: they come
/// from a throwaway harness run once during R29 Batch-2 over the
/// acceptance-① fixture and functions named here, and no test re-derives
/// them, so treat them as a recorded observation rather than a pinned
/// quantity. `render_baked_to_image(max_edge = k)` differs from
/// the same-resampler downscale of the full develop by a mean 5.7 codes8 at
/// k = 400 and 19.8 codes8 at k = 200, against 19.1 / 30.0 codes8 for the
/// entire effect of that recipe (neutral vs graded at the same size), with
/// a resampler-only control of 0.25 codes8. That is not a defect in either
/// path — normalised operators are SUPPOSED to place their halos at the
/// working resolution's scale, so the two disagree by construction — but it
/// is exactly why the delivery flag resizes finished pixels instead of
/// quietly reusing the preview path.
#[test]
fn the_develop_radius_is_a_fraction_of_the_frame_not_a_pixel_count() {
    // Clarity alone: its radius is the largest of the three, so the halo is
    // the easiest to measure, and its midtone weight is ~0.75 at both
    // plateau levels below (m = 1 − (2l − 1)²), so neither side is starved.
    let recipe = EditRecipe { clarity: 60.0, ..Default::default() };
    // Halo width in PIXELS: walk left from the step and count how far the
    // overshoot is still visible against the far-field plateau.
    let halo = |w: usize, h: usize| -> usize {
        let mut data: Vec<[f32; 3]> = (0..w * h)
            .map(|i| if i % w < w / 2 { [0.25; 3] } else { [0.75; 3] })
            .collect();
        apply_develop_anon(&mut data, w, h, &recipe);
        let row = h / 2;
        // The plateau as DEVELOPED (x = 0 is beyond any halo at these
        // radii), not the literal 0.25 — the tone stage is free to move it.
        let plateau = data[row * w][0];
        (0..w / 2)
            .rev()
            .take_while(|x| (data[row * w + x][0] - plateau).abs() > 1e-3)
            .count()
    };
    // 2 % of the short edge: 24 px at 1200, 12 px at 600 — both clear of
    // the 8 px floor, which would otherwise flatten the ratio by itself.
    let big = halo(1600, 1200);
    let small = halo(800, 600);
    assert!(big > 0 && small > 0, "clarity must produce a halo at all: {big} / {small}");
    // MEASURED: 59 px at 1600×1200, 30 px at 800×600 — ratio 1.967.
    let ratio = big as f64 / small as f64;
    assert!(
        (ratio - 2.0).abs() < 0.15,
        "clarity's radius must scale with the frame: halo {big} px at 1600×1200 vs \
         {small} px at 800×600 (ratio {ratio:.3}, expected 2.0 ± 0.15)"
    );
    // …and each halo really is the reach of a 2 %-of-short-edge blur, not
    // some other quantity that happens to double. Three box passes of
    // radius r reach 3r, so the ceiling is exact; the floor is 70 % of it
    // because the 1e-3 detection threshold above cuts the blur's thin outer
    // tail short (measured 59 of a possible 72, and 30 of 36 — the same
    // fraction at both sizes, which is itself the shape being preserved).
    // A constant radius fails this at 1600×1200 whatever it is set to.
    for (short, measured) in [(1200usize, big), (600, small)] {
        let radius = (0.02 * short as f64).round() as usize;
        let reach = 3 * radius;
        assert!(
            measured <= reach && measured * 10 >= reach * 7,
            "a {short} px short edge gives clarity a {radius} px radius, so the halo must \
             land in {}..={reach} px — measured {measured}",
            reach * 7 / 10
        );
    }
}

#[test]
fn kelvin_to_rgb_warm_is_redder_than_cool() {
    let warm = kelvin_to_rgb(3000.0);
    let cool = kelvin_to_rgb(9000.0);
    // STRICT: a kelvin_to_rgb that ignored its argument satisfied the
    // old >= / <= forms (R12).
    assert!(warm[0] > cool[0], "warm red {} > cool red {}", warm[0], cool[0]);
    assert!(warm[2] < cool[2], "warm blue {} < cool blue {}", warm[2], cool[2]);
}

/// The published Tanner-Helland constants have a cliff at the 6600 K
/// branch seam (green +1.31 %, blue +0.96 %) and a red plateau to 6688 K
/// where the r/b ratio — the eyedropper's temperature signal — does not
/// move at all. The recalibrated branches must be continuous at the seam
/// and alive inside the formerly dead band.
#[test]
fn the_6600k_branch_seam_is_continuous_and_carries_a_temperature_signal() {
    // C0 at the seam: 2 K apart may differ by slope (≈2e-4 per channel),
    // never by a branch cliff (the old green cliff alone was 1.3e-2).
    let below = kelvin_to_rgb(6599.0);
    let above = kelvin_to_rgb(6601.0);
    for c in 0..3 {
        assert!(
            (below[c] - above[c]).abs() < 2e-3,
            "channel {c} jumps across the seam: {} vs {}",
            below[c],
            above[c]
        );
    }
    // Inside 6600–6688 K the old red branch sat clamped at 255 while blue
    // was 255 by definition — r/b pinned at 1.0, so every temperature in
    // the band solved identically. The ratio must move now.
    let a = kelvin_to_rgb(6610.0);
    let b = kelvin_to_rgb(6680.0);
    let (ra, rb) = (a[0] / a[2], b[0] / b[2]);
    assert!(
        (ra - rb).abs() > 1e-3,
        "the r/b temperature signal is still dead in-band: {ra} vs {rb}"
    );
}

#[test]
fn wb_warmer_target_boosts_red_cuts_blue() {
    // Target warmer (higher K) than as-shot ⇒ Lightroom warms: red gain > 1, blue < 1.
    let g = wb_gains(5000.0, 7000.0, 0.0);
    assert!(g[0] > 1.0, "red gain {}", g[0]);
    assert!(g[2] < 1.0, "blue gain {}", g[2]);
    // Neutral (same K, no tint) ⇒ all gains ~1.
    let n = wb_gains(5500.0, 5500.0, 0.0);
    assert!((n[0] - 1.0).abs() < 1e-3 && (n[2] - 1.0).abs() < 1e-3);
}

#[test]
fn wb_eyedropper_neutralizes_a_synthetic_cast() {
    // Build the pixel a grey card shows under a known wrong WB: linear grey
    // L divided by the gains a (k0, tint0) correction WOULD apply — so that
    // correction is exactly what neutralises it. The solver must recover a
    // (k, tint) whose gains bring the pixel back to r≈g≈b, judged by the
    // same forward model (parameter identity is NOT required — nearby K
    // can neutralise equally well; neutrality is the contract).
    for (k0, tint0) in [(3200.0f32, 12.0f32), (7500.0, -18.0), (5500.0, 0.0)] {
        let g0 = wb_gains(5500.0, k0, tint0);
        let l = 0.18f32;
        let cast = [
            linear_to_srgb(l / g0[0]),
            linear_to_srgb(l / g0[1]),
            linear_to_srgb(l / g0[2]),
        ];
        let (k, tint) = solve_wb_from_neutral(cast, 5500.0);
        let g = wb_gains(5500.0, k, tint);
        let out = [
            srgb_to_linear(cast[0]) * g[0],
            srgb_to_linear(cast[1]) * g[1],
            srgb_to_linear(cast[2]) * g[2],
        ];
        let (mx, mn) = (
            out[0].max(out[1]).max(out[2]),
            out[0].min(out[1]).min(out[2]),
        );
        assert!(
            (mx - mn) / mx < 0.02,
            "cast for ({k0},{tint0}) not neutralised: solved ({k:.0},{tint:.1}) → {out:?}"
        );
    }
    // An already-neutral pixel solves to ~as-shot, ~zero tint.
    let (k, tint) = solve_wb_from_neutral([0.5, 0.5, 0.5], 5500.0);
    assert!((k - 5500.0).abs() < 300.0 && tint.abs() < 2.0, "neutral → ({k:.0},{tint:.1})");
}

#[test]
fn as_shot_math_lands_on_canonical_illuminants() {
    // Identity camera matrix ⇒ camera space IS XYZ; the WB gains
    // neutralise the illuminant, so wb = 1/XYZ reconstructs it exactly.
    let id = [[1.0f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    // D65 (X 0.95047, Y 1, Z 1.08883): McCamy ⇒ ~6504 K, and D65 sits
    // Duv ≈ +0.0032 ABOVE the Planckian locus ⇒ tint ≈ +10 — ACR's own
    // Daylight-preset tint, which pins the sign convention AND the scale.
    let (k, t) = wb_to_kelvin_tint(&id, [1.0 / 0.95047, 1.0, 1.0 / 1.08883]).unwrap();
    assert!((6400.0..=6600.0).contains(&k), "D65 → {k:.0} K");
    assert!((5.0..=15.0).contains(&t), "D65 → tint {t:+.1}");
    // Illuminant A (X 1.09850, Z 0.35585) lies ON the locus: ~2856 K, ~0.
    let (k, t) = wb_to_kelvin_tint(&id, [1.0 / 1.0985, 1.0, 1.0 / 0.35585]).unwrap();
    assert!((2790.0..=2920.0).contains(&k), "A → {k:.0} K");
    assert!(t.abs() < 6.0, "A → tint {t:+.1}");
    // Damaged coefficients refuse instead of anchoring nonsense.
    assert_eq!(wb_to_kelvin_tint(&id, [f32::NAN, 1.0, 1.0]), None);
    assert_eq!(wb_to_kelvin_tint(&id, [0.0, 1.0, 1.0]), None);
}

#[test]
fn stamped_as_shot_anchor_moves_only_kelvin_edits() {
    // tint-only renders identically stamped or legacy — the tint gain
    // never depends on the anchor — so no old archive can move.
    let grey = [[0.5f32, 0.5, 0.5]; 4];
    let mut legacy_px = grey;
    apply_recipe_wb(&mut legacy_px, &EditRecipe { tint: 20.0, ..Default::default() });
    let mut stamped_px = grey;
    apply_recipe_wb(
        &mut stamped_px,
        &EditRecipe { tint: 20.0, as_shot_k: Some(4000.0), ..Default::default() },
    );
    assert_eq!(legacy_px, stamped_px, "tint-only must ignore the anchor");
    // An ABSOLUTE target equal to the stamped as-shot is a true no-op —
    // the honest semantic the 5500-anchored model could not express…
    let mut at_as_shot = grey;
    apply_recipe_wb(
        &mut at_as_shot,
        &EditRecipe {
            temperature_k: Some(4000.0),
            as_shot_k: Some(4000.0),
            ..Default::default()
        },
    );
    assert_eq!(at_as_shot, grey, "target == as-shot must not shift");
    // …while a LEGACY recipe with the same numbers still takes its tuned
    // 5500-anchored shift, byte-identical to the old engine.
    let mut legacy_shift = grey;
    apply_recipe_wb(
        &mut legacy_shift,
        &EditRecipe { temperature_k: Some(4000.0), ..Default::default() },
    );
    assert_ne!(legacy_shift, grey, "legacy 5500-anchored shift still applies");
}

#[test]
fn export_refuses_a_recipe_whose_mask_raster_is_unreadable() {
    use crate::recipe::LocalAdjustment;
    let dir = std::env::temp_dir().join(format!("autoshade_maskgate_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("base.png");
    image::DynamicImage::new_rgb8(8, 8).save(&src).unwrap();
    let broken = LocalAdjustment {
        name: "sky".into(),
        amount: 1.0,
        exposure_ev: -0.5,
        mask: MaskGeometry::Bitmap { path: dir.join("gone.png").display().to_string() },
        ..Default::default()
    };
    let r = EditRecipe { masks: vec![broken.clone()], ..Default::default() };
    let out = dir.join("out.png");
    let err = render_to_file(&src, &r, &out, None, None, crate::diag::stderr()).unwrap_err().to_string();
    assert!(err.contains("sky"), "the refusal names the mask: {err}");
    assert!(!out.exists(), "a refused export writes nothing");
    // amount = 0 is inert BY the recipe — nothing is being dropped.
    let mut disabled = broken;
    disabled.amount = 0.0;
    let r = EditRecipe { masks: vec![disabled], ..Default::default() };
    render_to_file(&src, &r, &out, None, None, crate::diag::stderr()).expect("disabled mask exports fine");
    // A PARKED mask (default amount 1, every adjustment neutral) renders
    // nothing even with a healthy raster — its lost raster must not
    // block the export either.
    let parked = LocalAdjustment {
        name: "parked".into(),
        mask: MaskGeometry::Bitmap { path: dir.join("gone.png").display().to_string() },
        ..Default::default()
    };
    let r = EditRecipe { masks: vec![parked], ..Default::default() };
    render_to_file(&src, &r, &out, None, None, crate::diag::stderr()).expect("engine-inert mask exports fine");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn straighten_rotation_geometry_and_direction() {
    // (a) 0° is the identity (dims + pixels untouched). A non-uniform
    // frame and a byte comparison: the old uniform frame + dims-only
    // check passed even if the zero-angle branch returned cleared or
    // arbitrary pixels (R12).
    let img = DynamicImage::ImageRgb8(RgbImage::from_fn(40, 30, |x, y| {
        image::Rgb([(x * 6) as u8, (y * 8) as u8, ((x + y) % 256) as u8])
    }));
    let same = rotate_straighten(&img, 0.0);
    assert_eq!((same.width(), same.height()), (40, 30));
    assert_eq!(
        same.to_rgb8().as_raw(),
        img.to_rgb8().as_raw(),
        "0° must return the pixels untouched"
    );

    // (b) inscribed_dims: identity at 0°, symmetric in ±θ, strictly smaller
    // than the frame for any real tilt.
    assert_eq!(inscribed_dims(120.0, 80.0, 0.0), (120.0, 80.0));
    let (w1, h1) = inscribed_dims(120.0, 80.0, 7.0);
    let (w2, h2) = inscribed_dims(120.0, 80.0, -7.0);
    assert!((w1 - w2).abs() < 1e-4 && (h1 - h2).abs() < 1e-4);
    assert!(w1 < 120.0 && h1 < 80.0 && w1 > 90.0 && h1 > 60.0, "({w1},{h1})");

    // (c) No black corners: an all-white frame stays all-white after any
    // tilt — the auto-crop must keep every sample inside the source.
    let white = DynamicImage::ImageRgb8(RgbImage::from_pixel(120, 80, image::Rgb([255, 255, 255])));
    for deg in [3.0, 7.0, -12.0, 30.0] {
        let r = rotate_straighten(&white, deg).to_rgb8();
        let min = r.pixels().flat_map(|p| p.0).min().unwrap();
        assert!(min >= 250, "black bleed at {deg}°: min channel {min}");
    }

    // (d) Direction: positive = CLOCKWISE (the recipe contract). A vertical
    // red|blue split rotated clockwise tilts its divider top-to-the-right,
    // so just right of centre at the TOP row the red half now covers it.
    let mut split = RgbImage::new(100, 100);
    for (x, _y, p) in split.enumerate_pixels_mut() {
        *p = if x < 50 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) };
    }
    let rot = rotate_straighten(&DynamicImage::ImageRgb8(split), 10.0).to_rgb8();
    let (rw, _rh) = rot.dimensions();
    let probe = rot.get_pixel(rw / 2 + 3, 1);
    assert!(probe[0] > probe[2], "clockwise tilt must move red over top-centre-right: {probe:?}");
}

#[test]
fn distortion_maps_are_inverse_and_directionally_correct() {
    let dims = (120.0, 80.0);
    // (a) amount = 0 is the exact identity, both directions.
    assert_eq!(distort_norm(0.31, 0.77, dims, 0.0), (0.31, 0.77));
    assert_eq!(undistort_norm(0.31, 0.77, dims, 0.0), (0.31, 0.77));
    // (b) the centre is a fixed point at any amount.
    for amt in [-100.0f32, -45.0, 60.0, 100.0] {
        let (cx, cy) = distort_norm(0.5, 0.5, dims, amt);
        assert!((cx - 0.5).abs() < 1e-5 && (cy - 0.5).abs() < 1e-5, "centre moved at {amt}");
    }
    // (c) Round-trips. view→orig→view must hold everywhere in the frame;
    // orig→view→orig only for content the correction keeps (interior
    // points — a +100 barrel fix legitimately crops the outermost corners,
    // and those originals have no preimage by design).
    for amt in [-100.0f32, -45.0, 60.0, 100.0] {
        for (nx, ny) in [(0.0, 0.0), (1.0, 0.0), (0.1, 0.9), (0.3, 0.4), (0.62, 0.85), (0.5, 0.5)] {
            let (ox, oy) = distort_norm(nx, ny, dims, amt);
            let (bx, by) = undistort_norm(ox, oy, dims, amt);
            assert!(
                (bx - nx).abs() < 2e-3 && (by - ny).abs() < 2e-3,
                "view roundtrip @{amt}: ({nx},{ny}) → ({ox},{oy}) → ({bx},{by})"
            );
        }
        for (nx, ny) in [(0.3, 0.4), (0.6, 0.35), (0.25, 0.7), (0.45, 0.52)] {
            let (vx, vy) = undistort_norm(nx, ny, dims, amt);
            let (bx, by) = distort_norm(vx, vy, dims, amt);
            assert!(
                (bx - nx).abs() < 2e-3 && (by - ny).abs() < 2e-3,
                "orig roundtrip @{amt}: ({nx},{ny}) → ({vx},{vy}) → ({bx},{by})"
            );
        }
    }
    // (d) Direction, via the radial sampling ratio f = r_src/r_dst probed
    // along the x-axis: a barrel fix (+) pulls samples INWARD, harder at
    // the edge (f < 1, decreasing); a pincushion fix (−) samples RELATIVELY
    // further out at the edge than at the centre (f increasing).
    let ratio = |nx: f32, amt: f32| {
        let (ox, _) = distort_norm(nx, 0.5, dims, amt);
        (ox - 0.5) / (nx - 0.5)
    };
    assert!(
        ratio(0.95, 100.0) < ratio(0.6, 100.0) && ratio(0.6, 100.0) < 1.0,
        "barrel fix direction: f(edge)={} f(mid)={}",
        ratio(0.95, 100.0),
        ratio(0.6, 100.0)
    );
    assert!(
        ratio(0.95, -100.0) > ratio(0.6, -100.0),
        "pincushion fix direction: f(edge)={} f(mid)={}",
        ratio(0.95, -100.0),
        ratio(0.6, -100.0)
    );
}

#[test]
fn view_original_norm_maps_roundtrip_and_identity() {
    let dims = (1200.0, 800.0);
    let off = crate::recipe::LensProfile::default();
    // Identity when every control is zero.
    assert_eq!(view_to_original_norm(0.31, 0.77, dims, 0.0, &off, 0.0), (0.31, 0.77));
    assert_eq!(original_to_view_norm(0.31, 0.77, dims, 0.0, &off, 0.0), (0.31, 0.77));
    // Round-trip through straighten + manual distortion for interior
    // points (the composed map the web region box and every GUI mask
    // gesture ride on).
    for (deg, amt) in [(4.5f32, 0.0f32), (0.0, 35.0), (-3.0, -60.0), (7.0, 80.0)] {
        for (nx, ny) in [(0.3, 0.4), (0.55, 0.6), (0.42, 0.35), (0.5, 0.5)] {
            let (ox, oy) = view_to_original_norm(nx, ny, dims, deg, &off, amt);
            let (bx, by) = original_to_view_norm(ox, oy, dims, deg, &off, amt);
            assert!(
                (bx - nx).abs() < 3e-3 && (by - ny).abs() < 3e-3,
                "roundtrip deg={deg} amt={amt}: ({nx},{ny}) → ({ox},{oy}) → ({bx},{by})"
            );
            // A round trip alone proves only that the two maps invert
            // EACH OTHER: replace both with the identity and every
            // assertion above still passes while masks, brush strokes and
            // region boxes quietly stop tracking the displayed geometry.
            // So also require that active geometry actually MOVES an
            // off-centre point. (The frame centre is a fixed point of
            // both rotation and radial distortion — it is excluded.)
            if (deg != 0.0 || amt != 0.0) && (nx, ny) != (0.5, 0.5) {
                assert!(
                    (ox - nx).abs() > 1e-4 || (oy - ny).abs() > 1e-4,
                    "deg={deg} amt={amt} left ({nx},{ny}) unmoved — the map is inert"
                );
            }
        }
    }
}

/// **v1.5.0 F5**: Lightroom's two PROFILE strength sliders — and the
/// property that matters most about them, which is that 100 renders the
/// pre-v1.5.0 bytes EXACTLY. A strength that only nearly vanished at its
/// own neutral would rewrite every photo in the library the day it shipped.
#[test]
fn the_profile_strengths_scale_the_correction_and_100_changes_nothing() {
    use crate::recipe::LensProfile;
    let profile = LensProfile {
        vignette: (0..16).map(|i| 1.0 + 0.42 * (i as f32 / 15.0).powi(2)).collect(),
        distortion: (0..16).map(|i| 1.0008 - 0.053 * (i as f32 / 15.0).powi(2)).collect(),
        vignette_on: true,
        distortion_on: true,
        ..Default::default()
    };
    let base =
        DynamicImage::ImageRgb8(RgbImage::from_pixel(120, 80, image::Rgb([100, 100, 100])));
    let at = |vig: f32, dist: f32| EditRecipe {
        lens_profile: profile.clone(),
        lens_profile_vignetting_scale: vig,
        lens_profile_distortion_scale: dist,
        ..Default::default()
    };
    // (a) 100 is the identity, to the byte — and through the WHOLE tail, so
    // the distortion strength's effect on the resample counts too.
    let tail = |r: &EditRecipe| {
        frame_and_finish(
            develop_preview(&base, r),
            r,
            &geometry_profile(r),
            FilmScale::NATIVE,
            CropPolicy::Cut,
        )
        .to_rgb8()
    };
    let neutral = at(100.0, 100.0);
    let mut bare = neutral.clone();
    bare.lens_profile_vignetting_scale = 100.0;
    bare.lens_profile_distortion_scale = 100.0;
    assert_eq!(tail(&neutral).as_raw(), tail(&bare).as_raw(), "100 must be the identity");
    assert!(
        matches!(geometry_profile(&neutral), std::borrow::Cow::Borrowed(_)),
        "and at 100 with the CA pair at rest nothing is even allocated"
    );

    // (b) The VIGNETTING strength scales the lift: half at 50, none at 0,
    // and twice at 200. Read at a corner, where the gain is largest.
    let corner = |vig: f32| tail(&at(vig, 100.0))[(0, 0)][0] as f32;
    let (full, half, none, double) = (corner(100.0), corner(50.0), corner(0.0), corner(200.0));
    assert!(none < half && half < full && full < double, "{none} {half} {full} {double}");
    assert_eq!(none, 100.0, "0 is exactly no profile vignetting, so the corner is untouched");
    // HOW FAR is asserted on the gain LUT rather than on the rendered byte.
    // The claim is exact — half the slider, half the departure from 1 — and
    // an 8-bit corner cannot witness it: the byte is rounded, the transfer
    // pair is itself a LUT, and the two errors bias a RATIO of small
    // numbers by a couple of percent (measured: 0.477 where 0.5 is meant).
    // Ordering and the two endpoints above are exact and stay where the
    // photographer sees them; the arithmetic is checked where it lives.
    let gains = |s: f32| profile_vignette_lut(&profile.vignette, s);
    let (g100, g50, g200) = (gains(100.0), gains(50.0), gains(200.0));
    assert_eq!(
        g100,
        (0..LUT_N)
            .map(|i| profile_knot_interp(&profile.vignette, i as f32 / (LUT_N - 1) as f32))
            .collect::<Vec<_>>(),
        "100 hands the profile's own gains straight through"
    );
    for i in 0..LUT_N {
        assert!(
            ((g50[i] - 1.0) - (g100[i] - 1.0) * 0.5).abs() < 1e-6,
            "gain {i}: 50 must halve the lift, {} vs {}",
            g50[i],
            g100[i]
        );
        assert!(
            ((g200[i] - 1.0) - (g100[i] - 1.0) * 2.0).abs() < 1e-6,
            "gain {i}: 200 must double it, {} vs {}",
            g200[i],
            g100[i]
        );
    }
    assert!(gains(0.0).iter().all(|g| *g == 1.0), "0 is exactly no correction");

    // (c) The DISTORTION strength scales the resample, and 0 switches the
    // stage OFF rather than running an identity spline — which is visible
    // in the profile it composes, not just in the pixels.
    let zero = at(100.0, 0.0);
    let composed = geometry_profile(&zero);
    assert!(composed.distortion.is_empty(), "0 drops the knots: {:?}", composed.distortion);
    assert!(!composed.geometry_active(), "…so the resample does not run at all");
    let dist_only = |d: f32| {
        let r = EditRecipe {
            lens_profile: LensProfile { vignette_on: false, ..profile.clone() },
            lens_profile_distortion_scale: d,
            ..Default::default()
        };
        geometry_profile(&r).distortion.clone()
    };
    let (k100, k50) = (dist_only(100.0), dist_only(50.0));
    assert_eq!(k100, profile.distortion, "100 hands the profile's own knots straight through");
    for (i, (a, b)) in k50.iter().zip(&k100).enumerate() {
        assert!(
            ((a - 1.0) - (b - 1.0) * 0.5).abs() < 1e-6,
            "knot {i}: 50 must halve the DEPARTURE from identity, {a} vs {b}"
        );
    }
}

#[test]
fn lens_profile_vignette_lifts_corners_only_and_geometry_roundtrips() {
    use crate::recipe::LensProfile;
    // Real A7RIV-shaped data (P26 conversions): rising corner gains,
    // falling distortion factors (barrel), near-unity CA.
    let profile = LensProfile {
        vignette: (0..16).map(|i| 1.0 + 0.42 * (i as f32 / 15.0).powi(2)).collect(),
        distortion: (0..16).map(|i| 1.0008 - 0.053 * (i as f32 / 15.0).powi(2)).collect(),
        ca_r: vec![1.0005; 16],
        ca_b: vec![0.9995; 16],
        vignette_on: true,
        distortion_on: true,
        ca_on: true,
        // The mask warp plays no part in the PIXEL maps this test scores —
        // it is a mask-frame quantity and no resampler reads it.
        ..Default::default()
    };
    // (a) Vignette: corners brighten, the centre stays put.
    let base = DynamicImage::ImageRgb8(RgbImage::from_pixel(120, 80, image::Rgb([100, 100, 100])));
    let vig_only = EditRecipe {
        lens_profile: LensProfile { distortion_on: false, ca_on: false, ..profile.clone() },
        ..Default::default()
    };
    let out = develop_preview(&base, &vig_only).to_rgb8();
    assert_eq!(out[(60, 40)][0], 100, "centre untouched (gain 1.0)");
    assert!(out[(0, 0)][0] > 110, "corner lifted, got {}", out[(0, 0)][0]);

    // (b) Geometry: forward/inverse round-trip through the composed map.
    let dims = (1200.0, 800.0);
    for (nx, ny) in [(0.1, 0.1), (0.5, 0.2), (0.85, 0.7), (0.5, 0.5)] {
        let (ox, oy) = lens_geom_norm(nx, ny, dims, &profile, 20.0);
        let (bx, by) = lens_ungeom_norm(ox, oy, dims, &profile, 20.0);
        assert!(
            (bx - nx).abs() < 2e-3 && (by - ny).abs() < 2e-3,
            "roundtrip ({nx},{ny}) → ({ox},{oy}) → ({bx},{by})"
        );
    }
    // Inactive profile must be EXACTLY the manual path.
    let off = LensProfile::default();
    assert_eq!(lens_geom_norm(0.3, 0.8, dims, &off, 33.0), distort_norm(0.3, 0.8, dims, 33.0));

    // (c) Resample: a barrel profile pulls samples inward (like the manual
    // barrel fix) and leaves no unfilled pixels; identity-profile resample
    // with CA stays within a hair of the plain image.
    let white = DynamicImage::ImageRgb8(RgbImage::from_pixel(121, 81, image::Rgb([255, 255, 255])));
    let r = apply_lens_geometry(&white, &profile, 0.0).to_rgb16();
    let min = r.pixels().flat_map(|p| p.0).min().unwrap();
    assert!(min >= 65000, "unfilled pixels through the profile map: min {min}");
    // A flat frame cannot see WHERE samples come from — an identity
    // resampler passes the white probe (U14). Encode x into the value:
    // the barrel profile must pull the left edge inward (nonzero source
    // x → nonzero value), and CA must sample R and B at their own radii
    // (ca_r > 1 reaches farther out than green; ca_b < 1 less far).
    let ramp = DynamicImage::ImageRgb16(ImageBuffer::from_fn(121, 81, |x, _| {
        Rgb([(x as u16) * 500; 3])
    }));
    let g = apply_lens_geometry(&ramp, &profile, 0.0).to_rgb16();
    let p = g.get_pixel(0, 40).0;
    assert!(p[1] > 300, "profile distortion inert at the left edge: {p:?}");
    assert!(
        p[0] < p[1] && p[1] < p[2],
        "CA directions: ca_r > 1 samples farther out (smaller ramp value \
         at the left edge), ca_b < 1 nearer (larger) — a symmetric split \
         also passed a swapped R/B correction (Codex batch 40): {p:?}"
    );
}

/// The A7RIV frame every mask-frame measurement in R27 Batches 8-10 and
/// R29's `D` adjudication was made on.
const MASK_WARP_DIMS: (f32, f32) = (9504.0, 6336.0);

/// The 24 mm mask warp, from the SAME `.lcp` node the recon solved — one
/// fixture, so a change to either solver shows up as a disagreement rather
/// than as two independently drifting sets of numbers.
fn lcp_24mm_warp() -> Vec<f32> {
    let n = crate::lcp::PerspectiveModel {
        focal_mm: Some(24.0),
        focus_distance: Some(10000.0),
        scale: 1.027391,
        k: [-0.127336, 0.087661, -0.019675],
        focal_x: None,
        sensor_format_factor: 1.0,
        vignette: None,
    };
    n.mask_warp_knots(MASK_WARP_DIMS, crate::recipe::MASK_WARP_KNOTS).expect("solvable")
}

/// Source A and source B are two readings of ONE physical field, so they
/// have to agree — and the recon says by how much: the camera's own knot
/// map and Adobe's polynomial score 2.30 px and 2.11 px against the same
/// 138-patch pixel field, and differ from each other by 6.74 px rms over a
/// warp that reaches 185.7 px at the corner.
///
/// Asserted as a BAND, both ends. Too-large means one solver drifted; the
/// too-small end is the one that matters, because a source A that silently
/// became a copy of source B (or of the identity) would pass every other
/// test in this file.
#[test]
#[allow(clippy::excessive_precision)] // exact decoded 0x7037 fixture decimals
fn the_two_mask_warp_sources_agree_to_the_measured_tolerance() {
    // `exp_C_ref.ARW`'s OWN `0x7037` array, converted by `lensmeta`'s
    // `v·2⁻¹⁴ + 1` and dumped on this machine — the real A7RIV @ 24 mm
    // barrel profile, not a curve shaped like one.
    let camera_native: Vec<f32> = vec![
        1.0007934570,
        0.9998779297,
        0.9981079102,
        0.9959716797,
        0.9927368164,
        0.9890136719,
        0.9846191406,
        0.9800415039,
        0.9749145508,
        0.9696044922,
        0.9641113281,
        0.9589843750,
        0.9538574219,
        0.9492187500,
        0.9448242188,
        0.9412231445,
    ];
    let camera = crate::lensmeta::resample_sony_distortion(
        &camera_native,
        crate::lensmeta::SONY_DISTORTION_CANONICAL_KNOTS,
    );
    // The engine's own fill scale on that array, against the value the
    // recon computed independently: `profile_fill_scale` is the s_p this
    // whole inversion divides by, so a drift there is a silent drift in
    // every mask-warp knot below.
    assert!(
        (profile_fill_scale(&camera, MASK_WARP_DIMS) - 0.9755544).abs() < 1e-5,
        "fill scale {} vs the zero-parameter model's 0.9755544",
        profile_fill_scale(&camera, MASK_WARP_DIMS)
    );
    let a = mask_warp_from_camera_knots(
        &camera,
        MASK_WARP_DIMS,
        crate::recipe::MASK_WARP_KNOTS,
    );
    let b = lcp_24mm_warp();
    assert_eq!(a.len(), crate::recipe::MASK_WARP_KNOTS);
    assert_eq!(b.len(), crate::recipe::MASK_WARP_KNOTS);
    let half_diag = 0.5 * (MASK_WARP_DIMS.0.hypot(MASK_WARP_DIMS.1));
    let mut worst = 0.0f32;
    for i in 0..crate::recipe::MASK_WARP_KNOTS {
        let r = (i as f32 + 0.5) / (crate::recipe::MASK_WARP_KNOTS - 1) as f32 * half_diag;
        worst = worst.max(((a[i] - b[i]) * r).abs());
    }
    assert!(worst < 40.0, "the two sources diverged by {worst:.1} px");
    // PREMISE: both really are a warp, not the identity dressed up as one.
    for (name, k) in [("camera", &a), ("lcp", &b)] {
        let corner = (k[k.len() - 1] - 1.0) * half_diag;
        assert!(corner.abs() > 100.0, "{name} corner displacement {corner:.1} px is not a warp");
        assert!(k[0] < 0.995, "{name} centre magnification {} is not a warp", k[0]);
    }
}

/// ACCEPTANCE ⑦ (engine half). The forward map and its bisection inverse
/// compose to the identity across the frame — the property every consumer
/// of a coordinate map in this file is held to.
#[test]
fn the_mask_warp_point_map_round_trips() {
    let profile = crate::recipe::LensProfile {
        mask_warp: lcp_24mm_warp(),
        mask_warp_src: crate::recipe::MaskWarpSource::Lcp,
        ..Default::default()
    };
    let mut moved = 0.0f32;
    for i in 0..=10 {
        for j in 0..=10 {
            let (nx, ny) = (i as f32 / 10.0, j as f32 / 10.0);
            let (wx, wy) = lr_mask_warp_norm(nx, ny, MASK_WARP_DIMS, &profile);
            let (bx, by) = lr_mask_unwarp_norm(wx, wy, MASK_WARP_DIMS, &profile);
            assert!((bx - nx).abs() < 1e-4 && (by - ny).abs() < 1e-4, "({nx},{ny})");
            moved = moved.max(((wx - nx) * MASK_WARP_DIMS.0).abs());
        }
    }
    // PREMISE: a round trip through two identities also round-trips.
    assert!(moved > 50.0, "the map moved at most {moved:.1} px — it is not a warp");
    // The frame CENTRE is a fixed point of a radial map, exactly.
    assert_eq!(lr_mask_warp_norm(0.5, 0.5, MASK_WARP_DIMS, &profile), (0.5, 0.5));
}

fn d2_camera_profile(native: &[f32]) -> crate::recipe::LensProfile {
    let distortion = crate::lensmeta::resample_sony_distortion(
        native,
        crate::lensmeta::SONY_DISTORTION_CANONICAL_KNOTS,
    );
    crate::recipe::LensProfile {
        mask_warp: mask_warp_from_camera_knots(
            &distortion,
            MASK_WARP_DIMS,
            crate::recipe::MASK_WARP_KNOTS,
        ),
        mask_warp_src: crate::recipe::MaskWarpSource::CameraMetadata,
        mask_warp_center: Some(crate::recipe::MaskWarpCenter {
            stored_px: [4768.0, 3168.0],
            stored_dims: [9504.0, 6336.0],
        }),
        ..Default::default()
    }
}

#[allow(clippy::excessive_precision)]
fn d2_linear_wall_native() -> [f32; 16] {
    [
        1.0007934570,
        0.9998779297,
        0.9981079102,
        0.9959716797,
        0.9927368164,
        0.9890136719,
        0.9846191406,
        0.9800415039,
        0.9749145508,
        0.9696044922,
        0.9641113281,
        0.9589843750,
        0.9538574219,
        0.9492187500,
        0.9448242188,
        0.9412231445,
    ]
}

fn d2_linear_probe(
    zero: (f32, f32),
    full: (f32, f32),
) -> crate::recipe::LocalAdjustment {
    crate::recipe::LocalAdjustment {
        mask: MaskGeometry::Linear {
            zero_x: zero.0,
            zero_y: zero.1,
            full_x: full.0,
            full_y: full.1,
        },
        ..Default::default()
    }
}

fn d2_disabled_linear_profile(
    downstream_geometry: bool,
) -> (crate::recipe::LensProfile, crate::recipe::LensProfile) {
    let native = d2_linear_wall_native();
    let camera = d2_camera_profile(&native);
    let mut disabled = camera.clone();
    disabled.distortion = native.to_vec();
    disabled.distortion_on = downstream_geometry;
    disabled.linear_handle_warp = std::mem::take(&mut disabled.mask_warp);
    disabled.mask_warp_src = crate::recipe::MaskWarpSource::DisabledInSidecar;
    disabled.clamp();
    (camera, disabled)
}

fn d2_linear_handles(mask: &crate::recipe::LocalAdjustment) -> [(f32, f32); 2] {
    let MaskGeometry::Linear { zero_x, zero_y, full_x, full_y } = &mask.mask else {
        panic!("expected LINEAR fixture")
    };
    [(*zero_x, *zero_y), (*full_x, *full_y)]
}

fn d2_midline_x_at_y(handles: [(f32, f32); 2], y: f32, dims: (f32, f32)) -> f32 {
    let [(zx, zy), (fx, fy)] = handles.map(|(x, y)| (x * dims.0, y * dims.1));
    let (mx, my) = ((zx + fx) * 0.5, (zy + fy) * 0.5);
    mx - (y - my) * (fy - zy) / (fx - zx)
}

fn d2_midline_y_at_x(handles: [(f32, f32); 2], x: f32, dims: (f32, f32)) -> f32 {
    let [(zx, zy), (fx, fy)] = handles.map(|(x, y)| (x * dims.0, y * dims.1));
    let (mx, my) = ((zx + fx) * 0.5, (zy + fy) * 0.5);
    my - (x - mx) * (fx - zx) / (fy - zy)
}

/// The coverage the shipped LINEAR profile puts at the geometric midline.
/// `linear_coverage` is not symmetric about t = ½ (me6-2026-09 group B
/// measured the abscissa warp), so a bisection hunting for the midline
/// must hunt for THIS level rather than for a hard-coded 0.5.
fn d2_midline_coverage() -> f32 {
    linear_coverage(0.5, LINEAR_FALLOFF)
}

fn d2_coverage_crossing_x(
    mask: &crate::recipe::LocalAdjustment,
    y: f32,
    dims: (f32, f32),
) -> f32 {
    let ny = y / dims.1;
    let end = |nx| combined_mask_weight(mask, nx, ny, None, &[], None, dims);
    let increasing = end(1.0) > end(0.0);
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..40 {
        let mid = (lo + hi) * 0.5;
        if (end(mid) < d2_midline_coverage()) == increasing {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) * 0.5 * dims.0
}

fn d2_coverage_crossing_y(
    mask: &crate::recipe::LocalAdjustment,
    x: f32,
    dims: (f32, f32),
) -> f32 {
    let nx = x / dims.0;
    let end = |ny| combined_mask_weight(mask, nx, ny, None, &[], None, dims);
    let increasing = end(1.0) > end(0.0);
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..40 {
        let mid = (lo + hi) * 0.5;
        if (end(mid) < d2_midline_coverage()) == increasing {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) * 0.5 * dims.1
}

fn d2_gray_crossing_x(image: &image::GrayImage, y: u32) -> f32 {
    for x in 0..image.width() - 1 {
        let a = image.get_pixel(x, y)[0] as f32 - 127.5;
        let b = image.get_pixel(x + 1, y)[0] as f32 - 127.5;
        if a != b && a.signum() != b.signum() {
            return x as f32 + 0.5 + (-a) / (b - a);
        }
    }
    panic!("coverage row {y} never crossed 50%")
}

fn d2_rgb16_crossing_x_at(image: &DynamicImage, y: u32, target: f32) -> f32 {
    let image = image.to_rgb16();
    let mut range = (u16::MAX, u16::MIN);
    for x in 0..image.width() - 1 {
        range.0 = range.0.min(image.get_pixel(x, y)[1]);
        range.1 = range.1.max(image.get_pixel(x, y)[1]);
        let a = image.get_pixel(x, y)[1] as f32 - target;
        let b = image.get_pixel(x + 1, y)[1] as f32 - target;
        if a != b && a.signum() != b.signum() {
            return x as f32 + 0.5 + (-a) / (b - a);
        }
    }
    panic!("coverage row {y} range {range:?} never crossed target {target}")
}

#[test]
fn linear_with_active_camera_profile_lands_on_the_stored_corrected_frame_line() {
    let (w, h) = (1920u32, 1280u32);
    let dims = (w as f32, h as f32);
    let mut profile = d2_camera_profile(&d2_linear_wall_native());
    profile.distortion = d2_linear_wall_native().to_vec();
    profile.distortion_on = true;
    let mut mask = d2_linear_probe((0.33, 0.5), (0.27, 0.5));
    mask.exposure_ev = -4.0;
    let frame = MaskFrame::downstream(&profile, 0.0);
    assert!(frame.warps(), "premise: camera geometry must be active");

    // Engine-math pin: the output midline q asks the pre-geometry coverage
    // at its source p, and LINEAR's adapter must return q with no LR half.
    let q = (0.30f32, 0.50f32);
    let p = lens_geom_norm(q.0, q.1, dims, &profile, 0.0);
    let unwarp = frame.unwarp(dims).expect("active non-identity engine map");
    let weight = mask_weight_in(&mask.mask, p.0, p.1, None, Some(&unwarp), dims);
    // The midline's coverage is `d2_midline_coverage()`, not 0.5: the
    // shipped profile is eased on a warped abscissa. Dividing the coverage
    // error by the span rather than by the profile's own slope (1.535 at
    // the midline) makes this bound 1.5x TIGHTER than the pixel error it
    // names, which is the safe direction.
    let math_error_px = (weight - d2_midline_coverage()).abs() * 0.06 * dims.0;
    assert!(math_error_px < 0.1, "active LINEAR midline math is {math_error_px:.4}px off");

    // Raster pin: run the exact coverage-then-geometry composition used by
    // the GUI overlay and measure its 50% line in corrected output pixels.
    let base = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb([128, 128, 128])));
    let coverage = DynamicImage::ImageLuma8(mask_coverage(&mask, &base, frame));
    let rendered = apply_lens_geometry(&coverage, &profile, 0.0);
    // 255 codes then 257 per code: the same 8-bit ladder the old 32767.5
    // target rode, re-aimed at the midline's coverage (0.43837 -> 28728.6).
    let got = d2_rgb16_crossing_x_at(&rendered, h / 2, d2_midline_coverage() * 255.0 * 257.0);
    let expected = q.0 * dims.0;
    // The public coverage raster is 8-bit, so its rounded midline code adds
    // a measured sub-0.2 px crossing bias; the float engine-law assertion
    // above is the sub-0.1 px contract, while this pins end-to-end wiring.
    assert!((got - expected).abs() < 0.3, "rendered {got:.4}px vs stored {expected:.4}px");

    // The actual local-adjustment renderer uses the same frame preparation,
    // independently of the coverage-overlay entry point above.
    let recipe = EditRecipe {
        masks: vec![mask],
        lens_profile: profile.clone(),
        ..Default::default()
    };
    let effect = apply_lens_geometry(&develop_preview(&base, &recipe), &profile, 0.0);
    let effect_rgb = effect.to_rgb16();
    // The mask blend `p·(1 − w) + t·w` runs in the same sRGB-gamma domain
    // the output is rounded from, so the rendered value is AFFINE in the
    // coverage `w` and the midline's level is simply the two plateaus
    // interpolated at `d2_midline_coverage()`. This was the mean of the
    // two while the profile was symmetric about t = ½; it no longer is.
    // Interpolating (rather than reading the level off a probe render)
    // also keeps the target off the 8-bit ladder — on this 0.84-code-per-
    // pixel ramp a whole-code target is worth 0.6 px of bias.
    let zero_end = effect_rgb.get_pixel(w - 1, h / 2)[1] as f32;
    let full_end = effect_rgb.get_pixel(0, h / 2)[1] as f32;
    let target = zero_end + d2_midline_coverage() * (full_end - zero_end);
    let effect_crossing = d2_rgb16_crossing_x_at(&effect, h / 2, target);
    assert!(
        (effect_crossing - expected).abs() < 0.35,
        "active render {effect_crossing:.4}px vs stored {expected:.4}px"
    );
}

#[test]
fn linear_without_downstream_geometry_transports_all_wall_handle_pairs_forward() {
    let (camera, disabled) = d2_disabled_linear_profile(false);
    let (_, active_but_omitted) = d2_disabled_linear_profile(true);
    assert!(disabled.mask_warp.is_empty(), "RADIAL map must be disabled");
    assert_eq!(disabled.linear_handle_warp, camera.mask_warp);
    let frame = MaskFrame::downstream(&disabled, 0.0);
    let omitted_frame = MaskFrame::without_downstream(&active_but_omitted);
    assert!(!frame.warps(), "corrections-off fixture must have no downstream geometry");
    assert!(!omitted_frame.warps(), "the caller explicitly omitted downstream geometry");

    let fixtures = [
        ("L1", (0.33, 0.50), (0.27, 0.50), true, 3168.0),
        ("L2", (0.69, 0.50), (0.75, 0.50), true, 3168.0),
        ("L3", (0.50, 0.27), (0.50, 0.21), false, 4752.0),
    ];
    for (name, zero, full, vertical, along) in fixtures {
        let stored = d2_linear_probe(zero, full);
        let rendered = frame.linear_handles_to_raw(&stored, MASK_WARP_DIMS);
        let got_handles = d2_linear_handles(rendered.as_ref());
        let omitted = omitted_frame.linear_handles_to_raw(&stored, MASK_WARP_DIMS);
        assert_eq!(
            d2_linear_handles(omitted.as_ref()),
            got_handles,
            "{name}: explicit no-downstream path disagrees with inactive profile"
        );
        let expected_handles = [zero, full].map(|(x, y)| {
            lr_mask_unwarp_norm(x, y, MASK_WARP_DIMS, &camera)
        });
        for (which, (got, expected)) in got_handles.iter().zip(expected_handles).enumerate() {
            let error = ((got.0 - expected.0) * MASK_WARP_DIMS.0)
                .hypot((got.1 - expected.1) * MASK_WARP_DIMS.1);
            assert!(error < 0.01, "{name} handle {which} is {error:.5}px off D_fwd");
        }

        let (got, expected, stored_midline) = if vertical {
            (
                d2_coverage_crossing_x(rendered.as_ref(), along, MASK_WARP_DIMS),
                d2_midline_x_at_y(expected_handles, along, MASK_WARP_DIMS),
                (zero.0 + full.0) * 0.5 * MASK_WARP_DIMS.0,
            )
        } else {
            (
                d2_coverage_crossing_y(rendered.as_ref(), along, MASK_WARP_DIMS),
                d2_midline_y_at_x(expected_handles, along, MASK_WARP_DIMS),
                (zero.1 + full.1) * 0.5 * MASK_WARP_DIMS.1,
            )
        };
        assert!((got - expected).abs() < 0.1, "{name}: render {got:.4}px vs H2 {expected:.4}px");
        let delta = got - stored_midline;
        let expected_delta = match name {
            "L1" => -29.882,
            "L2" => 28.743,
            "L3" => -30.713,
            _ => unreachable!(),
        };
        assert!(
            (delta - expected_delta).abs() < 1.5,
            "{name}: displacement {delta:.3}px, expected {expected_delta:.3}±1.5px"
        );
    }
}

#[test]
fn linear_off_path_rendered_boundary_stays_straight_instead_of_h1_bowing() {
    let (camera, disabled) = d2_disabled_linear_profile(false);
    let (w, h) = (1188u32, 792u32);
    let dims = (w as f32, h as f32);
    let mut mask = d2_linear_probe((0.33, 0.5), (0.27, 0.5));
    mask.exposure_ev = -4.0;
    let frame = MaskFrame::downstream(&disabled, 0.0);
    let coverage = mask_coverage(&mask, &DynamicImage::new_rgb8(w, h), frame);
    let rows = [h / 10, h / 2, h - h / 10 - 1];
    let crossings = rows.map(|y| d2_gray_crossing_x(&coverage, y));
    let sag = crossings[1] - 0.5 * (crossings[0] + crossings[2]);
    assert!(sag.abs() < 0.5, "H2 rendered boundary sagged {sag:.3}px: {crossings:?}");

    let base = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb([128, 128, 128])));
    let recipe = EditRecipe {
        masks: vec![mask],
        lens_profile: disabled.clone(),
        ..Default::default()
    };
    let effect = develop_preview(&base, &recipe);
    let effect_rgb = effect.to_rgb16();
    let effect_crossings = rows.map(|y| {
        let target = 0.5
            * (effect_rgb.get_pixel(0, y)[1] as f32
                + effect_rgb.get_pixel(w - 1, y)[1] as f32);
        d2_rgb16_crossing_x_at(&effect, y, target)
    });
    for (y, (got, coverage_got)) in rows.into_iter().zip(effect_crossings.into_iter().zip(crossings)) {
        assert!(
            (got - coverage_got).abs() < 0.5,
            "row {y}: local render {got:.3}px vs coverage {coverage_got:.3}px"
        );
    }

    // Adversarial control: pointwise H1 on this same fixture bows by
    // multiple working-frame pixels, so the straightness gate is not an
    // axis-aligned identity test that both topologies can pass.
    let h1_crossing = |raw_y: f32| {
        let target_y = raw_y / dims.1;
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..40 {
            let mid = (lo + hi) * 0.5;
            if lr_mask_unwarp_norm(0.30, mid, dims, &camera).1 < target_y {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lr_mask_unwarp_norm(0.30, (lo + hi) * 0.5, dims, &camera).0 * dims.0
    };
    let h1 = rows.map(|y| h1_crossing(y as f32 + 0.5));
    let h1_sag = h1[1] - 0.5 * (h1[0] + h1[2]);
    assert!(
        h1_sag.abs() > 1.5,
        "premise: pointwise H1 sag is only {h1_sag:.3}px on {h1:?}"
    );
}

#[test]
fn radial_with_disabled_profile_and_retained_linear_map_stays_at_stored_coordinates() {
    let (_, disabled) = d2_disabled_linear_profile(true);
    assert!(disabled.geometry_active(), "premise: downstream geometry is active");
    assert!(disabled.mask_warp.is_empty(), "disabled RADIAL path must be identity");
    assert!(!disabled.linear_handle_warp.is_empty(), "premise: LINEAR map was retained");

    let (w, h) = (1920u32, 1280u32);
    let (cx, cy) = (0.30f32, 0.50f32);
    let base = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb([128, 128, 128])));
    let recipe = EditRecipe {
        masks: vec![probe_radial(cx, cy, 0.025)],
        lens_profile: disabled.clone(),
        ..Default::default()
    };
    let rendered = apply_lens_geometry(&develop_preview(&base, &recipe), &disabled, 0.0);
    let got = effect_centroid(&rendered);
    let expected = (cx as f64 * w as f64, cy as f64 * h as f64);
    let error = (got.0 - expected.0).hypot(got.1 - expected.1);
    assert!(error < 1.0, "disabled RADIAL moved {error:.3}px: {got:?} vs {expected:?}");
}

#[test]
#[allow(clippy::excessive_precision)] // exact D2 knot/vector fixture decimals
fn d2_zero_parameter_camera_model_closes_all_41_measured_vectors() {
    const WALL_NATIVE: [f32; 16] = [
        1.0007934570,
        0.9998779297,
        0.9981079102,
        0.9959716797,
        0.9927368164,
        0.9890136719,
        0.9846191406,
        0.9800415039,
        0.9749145508,
        0.9696044922,
        0.9641113281,
        0.9589843750,
        0.9538574219,
        0.9492187500,
        0.9448242188,
        0.9412231445,
    ];
    const DSC_NATIVE: [f32; 16] = [
        1.0007934570,
        0.9998779297,
        0.9982299805,
        0.9961547852,
        0.9932250977,
        0.9898071289,
        0.9856567383,
        0.9813842773,
        0.9766235352,
        0.9719238281,
        0.9668579102,
        0.9622802734,
        0.9576416016,
        0.9538574219,
        0.9503173828,
        0.9478149414,
    ];
    type Row = (&'static str, (f32, f32), (f32, f32));
    const WALL: [Row; 20] = [
        ("G1", (1710.72, 1267.20), (-20.897, -12.931)),
        ("G2", (4752.00, 1267.20), (-0.011, 31.992)),
        ("G3", (7793.28, 1267.20), (19.025, -12.135)),
        ("G4", (1710.72, 3168.00), (4.924, -0.025)),
        ("G5", (4752.00, 3168.00), (0.244, -0.019)),
        ("G6", (7793.28, 3168.00), (-6.669, 0.026)),
        ("G7", (1710.72, 5068.80), (-20.836, 12.932)),
        ("G8", (4752.00, 5068.80), (0.097, -31.970)),
        ("G9", (7793.28, 5068.80), (19.105, 12.141)),
        ("R1", (2851.20, 2027.52), (24.874, 14.890)),
        ("R2", (6652.80, 3991.68), (-28.390, -12.427)),
        ("centre_S", (4752.00, 3168.00), (0.535, 0.035)),
        ("centre_M", (4752.00, 3168.00), (0.393, 0.009)),
        ("centre_L", (4752.00, 3168.00), (0.303, 0.010)),
        ("edge_S", (1710.72, 3168.00), (4.962, -0.040)),
        ("edge_M", (1710.72, 3168.00), (5.085, -0.083)),
        ("edge_L", (1710.72, 3168.00), (4.990, 0.037)),
        ("corner_S", (1710.72, 1267.20), (-20.927, -12.969)),
        ("corner_M", (1710.72, 1267.20), (-20.710, -12.895)),
        ("corner_L", (1710.72, 1267.20), (-20.880, -12.903)),
    ];
    const DSC: [Row; 21] = [
        ("G1", (1710.72, 1267.20), (-18.900, -11.770)),
        ("G2", (4752.00, 1267.20), (0.140, 29.250)),
        ("G3", (7793.28, 1267.20), (17.480, -11.020)),
        ("G4", (1710.72, 3168.00), (4.610, -0.010)),
        ("G5", (4752.00, 3168.00), (0.300, -0.070)),
        ("G6", (7793.28, 3168.00), (-5.920, -0.020)),
        ("G7", (1710.72, 5068.80), (-18.360, 11.960)),
        ("G8", (4752.00, 5068.80), (0.230, -29.280)),
        ("G9", (7793.28, 5068.80), (17.570, 11.000)),
        ("original", (2283.03, 951.41), (-5.820, -5.100)),
        ("R1", (2851.20, 2027.52), (22.950, 13.600)),
        ("R2", (6652.80, 3991.68), (-26.060, -11.330)),
        ("centre_S", (4752.00, 3168.00), (0.620, -0.080)),
        ("centre_M", (4752.00, 3168.00), (0.420, -0.030)),
        ("centre_L", (4752.00, 3168.00), (0.400, 0.040)),
        ("edge_S", (1710.72, 3168.00), (4.760, -0.050)),
        ("edge_M", (1710.72, 3168.00), (4.380, -0.030)),
        ("edge_L", (1710.72, 3168.00), (4.550, -0.070)),
        ("corner_S", (1710.72, 1267.20), (-18.790, -11.690)),
        ("corner_M", (1710.72, 1267.20), (-18.740, -11.730)),
        ("corner_L", (1710.72, 1267.20), (-18.930, -11.730)),
    ];

    let check = |label: &str, native: &[f32], rows: &[Row], expected_rms: f32| {
        let profile = d2_camera_profile(native);
        let mut sum_sq = 0.0f32;
        let mut worst = 0.0f32;
        for &(cell, (x, y), (mx, my)) in rows {
            let (wx, wy) =
                lr_mask_warp_norm(x / MASK_WARP_DIMS.0, y / MASK_WARP_DIMS.1, MASK_WARP_DIMS, &profile);
            let (px, py) = (wx * MASK_WARP_DIMS.0 - x, wy * MASK_WARP_DIMS.1 - y);
            let err = (px - mx).hypot(py - my);
            sum_sq += err * err;
            worst = worst.max(err);
            assert!(err <= 1.0, "{label}/{cell}: predicted ({px},{py}), measured ({mx},{my}), error {err}");
        }
        let rms = (sum_sq / rows.len() as f32).sqrt();
        assert!((rms - expected_rms).abs() < 0.02, "{label}: rms {rms}, max {worst}");
    };
    check("wall", &WALL_NATIVE, &WALL, 0.568);
    check("P26", &DSC_NATIVE, &DSC, 0.243);
}

#[test]
fn mask_warp_uses_the_full_raw_centre_in_stored_coordinates() {
    let shifted = crate::recipe::LensProfile {
        mask_warp: vec![1.05; crate::recipe::MASK_WARP_KNOTS],
        mask_warp_src: crate::recipe::MaskWarpSource::CameraMetadata,
        mask_warp_center: Some(crate::recipe::MaskWarpCenter {
            stored_px: [4768.0, 3168.0],
            stored_dims: [9504.0, 6336.0],
        }),
        ..Default::default()
    };
    let fixed = (4768.0 / MASK_WARP_DIMS.0, 3168.0 / MASK_WARP_DIMS.1);
    assert_eq!(lr_mask_warp_norm(fixed.0, fixed.1, MASK_WARP_DIMS, &shifted), fixed);
    let stored_centre = lr_mask_warp_norm(0.5, 0.5, MASK_WARP_DIMS, &shifted);
    assert!(stored_centre.0 < 0.5, "shifted centre was ignored: {stored_centre:?}");

    // The same stored-pixel centre scales with a working-resolution
    // preview instead of remaining thousands of pixels off-frame.
    let preview_dims = (950.4, 633.6);
    let preview_fixed = (476.8 / preview_dims.0, 316.8 / preview_dims.1);
    assert_eq!(
        lr_mask_warp_norm(preview_fixed.0, preview_fixed.1, preview_dims, &shifted),
        preview_fixed
    );

    let legacy = crate::recipe::LensProfile { mask_warp_center: None, ..shifted };
    assert_eq!(lr_mask_warp_norm(0.5, 0.5, MASK_WARP_DIMS, &legacy), (0.5, 0.5));
}

#[test]
fn mask_frame_composes_the_engine_map_with_the_lr_inverse_exactly_once() {
    let profile = crate::recipe::LensProfile {
        distortion: (0..16).map(|i| 1.0 - 0.12 * (i as f32 / 15.0).powi(2)).collect(),
        distortion_on: true,
        mask_warp: (0..crate::recipe::MASK_WARP_KNOTS)
            .map(|i| 0.98 + 0.05 * i as f32 / (crate::recipe::MASK_WARP_KNOTS - 1) as f32)
            .collect(),
        mask_warp_src: crate::recipe::MaskWarpSource::CameraMetadata,
        mask_warp_center: Some(crate::recipe::MaskWarpCenter {
            stored_px: [4768.0, 3168.0],
            stored_dims: [9504.0, 6336.0],
        }),
        ..Default::default()
    };
    let dims = MASK_WARP_DIMS;
    let u = MaskUnwarp::new(&profile, 0.0, dims).expect("both maps are active");
    for (nx, ny) in [(0.15, 0.2), (0.5, 0.5), (0.82, 0.74)] {
        let engine = lens_ungeom_norm(nx, ny, dims, &profile, 0.0);
        let expected = lr_mask_unwarp_norm(engine.0, engine.1, dims, &profile);
        let got = u.at(nx, ny);
        assert!((got.0 - expected.0).abs() < 2e-5 && (got.1 - expected.1).abs() < 2e-5);
    }
}

/// ACCEPTANCE ⑤. With no solved warp the map is the IDENTITY — bit-for-bit,
/// not approximately — and the reason is available by name rather than
/// inferred from an empty vector.
#[test]
fn an_absent_profile_is_an_identity_warp_with_a_named_reason() {
    use crate::recipe::MaskWarpSource as S;
    let none = crate::recipe::LensProfile::default();
    assert_eq!(none.mask_warp_src, S::Absent);
    for i in 0..=7 {
        for j in 0..=7 {
            let (nx, ny) = (i as f32 / 7.0, j as f32 / 7.0);
            assert_eq!(lr_mask_warp_norm(nx, ny, MASK_WARP_DIMS, &none), (nx, ny));
            assert_eq!(lr_mask_unwarp_norm(nx, ny, MASK_WARP_DIMS, &none), (nx, ny));
        }
    }
    // Every "no warp" state names itself, and none of them is silent.
    for s in S::ALL {
        assert!(!s.en().is_empty(), "{s:?} has no prose");
        let mut p = crate::recipe::LensProfile { mask_warp_src: s, ..Default::default() };
        // A tag that is not SOLVED cannot keep knots — `clamp` enforces it,
        // so a hand-edited recipe cannot claim "fisheye refused" and warp.
        p.mask_warp = vec![1.02; 16];
        p.clamp();
        if s.is_solved() {
            assert_eq!(p.mask_warp.len(), 16, "{s:?} is a solved source");
        } else {
            assert!(p.mask_warp.is_empty(), "{s:?} kept knots it has no claim to");
            assert_eq!(lr_mask_warp_norm(0.3, 0.7, MASK_WARP_DIMS, &p), (0.3, 0.7));
        }
    }
    // Empty knots in, empty out: "no data" is not a warp of 1.0.
    assert!(mask_warp_from_camera_knots(&[], MASK_WARP_DIMS, 16).is_empty());
}

/// A strong, real-shaped barrel profile for the frame-wiring tests:
/// falling radius factors, the shape every Sony `0x7037` array has.
fn barrel_profile() -> crate::recipe::LensProfile {
    crate::recipe::LensProfile {
        distortion: (0..16).map(|i| 1.0 - 0.12 * (i as f32 / 15.0).powi(2)).collect(),
        distortion_on: true,
        ..Default::default()
    }
}

/// A hard-edged radial at a KNOWN stored centre, dark enough to find.
fn probe_radial(cx: f32, cy: f32, r: f32) -> crate::recipe::LocalAdjustment {
    crate::recipe::LocalAdjustment {
        mask: MaskGeometry::Radial {
            top: cy - r,
            left: cx - r,
            bottom: cy + r,
            right: cx + r,
            feather: 0.0,
            roundness: 0.0,
            flipped: false,
            angle: 0.0,
            midpoint: 50.0,
            mask_version: 0,
        },
        exposure_ev: -4.0,
        ..Default::default()
    }
}

/// Centroid of the darkened pixels, in output-frame pixels.
fn effect_centroid(img: &DynamicImage) -> (f64, f64) {
    let g = img.to_rgb8();
    let (mut sx, mut sy, mut n) = (0.0f64, 0.0f64, 0.0f64);
    for (x, y, p) in g.enumerate_pixels() {
        if p.0[1] < 90 {
            sx += x as f64;
            sy += y as f64;
            n += 1.0;
        }
    }
    assert!(n > 40.0, "the mask must darken a real region, got {n} px");
    (sx / n, sy / n)
}


/// ACCEPTANCE ⑥, REWRITTEN by the 2026-08-20 user ruling — the
/// PARAMETRIC-LANDS-ON-STORED-COORDINATES property.
///
/// # What this replaces, and why
///
/// Its predecessor pinned the opposite: that a radial's rendered weight is
/// computed at its stored coordinates in the PRE-geometry frame and never
/// touched. That was the shipped behaviour and it was wrong, because this
/// engine then resampled the whole frame and carried the mask with it —
/// while Lightroom does not move a parametric shape at all. The `D`
/// adjudication measured the gap on the 105 mm pair: the PIXELS move
/// +87.5 px at r ≈ 3250 (the `.lcp` model at 2.69 px rms, 30 NCC points,
/// tangential rms 1.22 px) while the radial mask measures a similarity of
/// 0.99956 — the identity to 0.05 %, and **88.7 px away from the pixel
/// field** (rms; 89.56 px max). That 88.7 px now supports THIS direction.
///
/// # The property, end to end
///
/// A radial at a known stored centre, developed and then resampled through
/// an active geometry stage — the real composition, `develop_preview` then
/// `apply_lens_geometry`, which is the same order `render_to_file` runs —
/// must put its darkened region back on the STORED centre.
///
/// The control in the same test provenances the tolerance: the identical
/// chain with [`MaskFrame::AsRendered`] (i.e. the behaviour before this
/// wiring) displaces the effect by the field, and that displacement is
/// asserted to be an order larger than the tolerance — so a passing test
/// cannot be a test of nothing.
///
/// MUTATION THIS KILLS: dropping or retargeting the explicit RADIAL/LINEAR
/// arms in `mask_weight_in` so RADIAL no longer uses `MaskUnwarp::at` or
/// LINEAR no longer uses `MaskUnwarp::engine_at`.
#[test]
fn a_parametric_mask_lands_on_its_stored_coordinates_under_lens_geometry() {
    let (w, h) = (960u32, 640u32);
    let base = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb([128, 128, 128])));
    // A stronger barrel than `barrel_profile`, so the field is many pixels
    // rather than one: corner factor 0.75, fill scale 0.923.
    let profile = crate::recipe::LensProfile {
        distortion: (0..16).map(|i| 1.0 - 0.25 * (i as f32 / 15.0).powi(2)).collect(),
        distortion_on: true,
        ..Default::default()
    };
    // TWO placements at different radii. The frame centre is a fixed point
    // of every radial map, so one off-centre mask could pass by accident of
    // where it sat; two at different radii cannot.
    //
    // MEASURED on this fixture (the numbers the tolerances below come from,
    // scanned over frame size x barrel strength x placement):
    //
    //   | stored cx | wired error | UNWIRED drift |
    //   |-----------|-------------|---------------|
    //   | 0.10      | 0.30 px     | 23.84 px      |
    //   | 0.32      | 0.09 px     |  8.39 px      |
    //
    // The sub-pixel residue is resampling, not geometry: the effect is a
    // filled disc whose centroid is recovered from 8-bit thresholded
    // pixels, and the bilinear resample softens its rim.
    for (cx, min_drift) in [(0.10f32, 15.0f64), (0.32f32, 5.0f64)] {
        let cy = 0.5f32;
        let recipe = EditRecipe {
            masks: vec![probe_radial(cx, cy, 0.05)],
            lens_profile: profile.clone(),
            ..Default::default()
        };
        let want = (cx as f64 * w as f64, cy as f64 * h as f64);

        // WIRED: `develop_preview` derives `WarpedDownstream` from the
        // recipe, exactly as the GUI and web preview surfaces do before
        // they warp, and the same decision `render_to_file` makes.
        let wired = apply_lens_geometry(&develop_preview(&base, &recipe), &profile, 0.0);
        let got = effect_centroid(&wired);

        // CONTROL: the identical chain with the adaptation switched off —
        // the behaviour before this wiring, and where the tolerance's
        // provenance comes from.
        let unwired = apply_lens_geometry(
            &develop_preview_framed(
                &base,
                &recipe,
                &crate::diag::pixels(),
                MaskFrame::AsRendered,
                None,
                false,
            ),
            &profile,
            0.0,
        );
        let drifted = effect_centroid(&unwired);

        let err = ((got.0 - want.0).powi(2) + (got.1 - want.1).powi(2)).sqrt();
        let drift = ((drifted.0 - want.0).powi(2) + (drifted.1 - want.1).powi(2)).sqrt();
        // PREMISE: the field really does move this mask, or the assertion
        // below is satisfied by an identity map and proves nothing.
        assert!(
            drift > min_drift,
            "cx={cx}: the control must displace by the field; it moved \
             {drift:.2} px (centroid {drifted:?} vs stored {want:?})"
        );
        assert!(
            err < 1.5,
            "cx={cx}: the wired chain must land on the STORED centre: \
             {err:.2} px off (centroid {got:?} vs stored {want:?}; \
             control drifts {drift:.2} px)"
        );
        assert!(
            err * 5.0 < drift,
            "cx={cx}: the fix must be an order better: {err:.2} vs {drift:.2}"
        );
    }
}

/// D2 continuation of the wired-centroid test above: when the Lightroom
/// transport is non-identity, the same exact-once engine cancellation must
/// land on `m_lr(stored)`, not on the stored point and not on a second copy
/// of either lens field.
#[test]
#[allow(clippy::excessive_precision)] // exact decoded 0x7037 fixture decimals
fn d2_wired_camera_mask_lands_at_the_lr_transported_coordinate() {
    const WALL_NATIVE: [f32; 16] = [
        1.0007934570,
        0.9998779297,
        0.9981079102,
        0.9959716797,
        0.9927368164,
        0.9890136719,
        0.9846191406,
        0.9800415039,
        0.9749145508,
        0.9696044922,
        0.9641113281,
        0.9589843750,
        0.9538574219,
        0.9492187500,
        0.9448242188,
        0.9412231445,
    ];
    let (w, h) = (1920u32, 1280u32);
    let base = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb([128, 128, 128])));
    let mut profile = d2_camera_profile(&WALL_NATIVE);
    // The render-side adjudication retained this established 16-knot
    // calibration; only the mask solve uses the corrected dense spline.
    profile.distortion = WALL_NATIVE.to_vec();
    profile.distortion_on = true;

    for cx in [0.10f32, 0.32f32] {
        let cy = 0.5f32;
        let recipe = EditRecipe {
            masks: vec![probe_radial(cx, cy, 0.025)],
            lens_profile: profile.clone(),
            ..Default::default()
        };
        let target = lr_mask_warp_norm(cx, cy, (w as f32, h as f32), &profile);
        let want = (target.0 as f64 * w as f64, target.1 as f64 * h as f64);
        let got = effect_centroid(&apply_lens_geometry(
            &develop_preview(&base, &recipe),
            &profile,
            0.0,
        ));
        let err = (got.0 - want.0).hypot(got.1 - want.1);
        let transport = (want.0 - cx as f64 * w as f64)
            .hypot(want.1 - cy as f64 * h as f64);
        eprintln!(
            "D2 wired centroid cx={cx:.2}: error={err:.3}px, Lightroom transport={transport:.3}px"
        );
        assert!(transport > 1.0, "premise: D2 transport is only {transport:.3}px");
        assert!(err < 1.0, "cx={cx}: centroid {got:?}, target {want:?}, error {err:.3}px");
    }
}

/// The COMPANION pin: with the geometry stage inactive, the mask chain is
/// unchanged — bit for bit, not approximately.
///
/// This is the other half of the "mask map and pixel warp travel together"
/// invariant, and the half that protects every photograph that has no lens
/// profile and no manual distortion (every non-Sony frame, and every Sony
/// frame whose photographer switched the correction off). The wiring must
/// cost them nothing at all.
///
/// MUTATION THIS KILLS: making `MaskFrame::downstream` return
/// `WarpedDownstream` unconditionally, or dropping `MaskUnwarp::new`'s
/// identity check.
#[test]
fn with_the_geometry_stage_inactive_the_mask_chain_is_untouched() {
    let (w, h) = (240u32, 160u32);
    let base = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb([128, 128, 128])));
    let masks = vec![probe_radial(0.24, 0.5, 0.1)];
    // (a) No profile at all, and a profile whose data is present but
    //     TOGGLED OFF — both are inert, and the second is the one a naive
    //     `!profile.distortion.is_empty()` gate would get wrong.
    let none = EditRecipe { masks: masks.clone(), ..Default::default() };
    let toggled_off = EditRecipe {
        lens_profile: crate::recipe::LensProfile {
            distortion_on: false,
            ..barrel_profile()
        },
        ..none.clone()
    };
    let a = develop_preview(&base, &none);
    let b = develop_preview(&base, &toggled_off);
    assert_eq!(a.to_rgb8().as_raw(), b.to_rgb8().as_raw(), "an inert profile moved a mask");
    // …and both equal the explicitly-unadapted chain, which is what
    // "unchanged from before this batch" means.
    let c = develop_preview_framed(&base, &none, &crate::diag::pixels(), MaskFrame::AsRendered, None, false);
    assert_eq!(a.to_rgb8().as_raw(), c.to_rgb8().as_raw());

    // (a2) The case the identity short-circuit in `MaskUnwarp::new` exists
    //      for, and the one `downstream` cannot catch: a profile that IS
    //      active by the gate — sixteen knots, toggle on — whose map is
    //      nevertheless the identity, because the lens has no distortion to
    //      correct. `geometry_active()` is true, so the frame says
    //      `WarpedDownstream`; the map must then recognise itself as inert
    //      and return `None` rather than push every mask coordinate through
    //      a float round trip.
    let flat = EditRecipe {
        lens_profile: crate::recipe::LensProfile {
            distortion: vec![1.0; 16],
            distortion_on: true,
            ..Default::default()
        },
        ..none.clone()
    };
    assert!(
        MaskFrame::downstream(&flat.lens_profile, 0.0).warps(),
        "premise: a flat profile is still ACTIVE by the gate"
    );
    assert!(
        MaskUnwarp::new(&flat.lens_profile, 0.0, (w as f32, h as f32)).is_none(),
        "an identity map must short-circuit, not round-trip every coordinate"
    );
    let d = develop_preview(&base, &flat);
    assert_eq!(
        a.to_rgb8().as_raw(),
        d.to_rgb8().as_raw(),
        "a distortion-free lens profile moved a mask"
    );

    // (b) The decision itself, at the source: an inert profile answers
    //     `AsRendered`, so the geometry stage is not run either.
    let inert = crate::recipe::LensProfile::default();
    assert!(!MaskFrame::downstream(&inert, 0.0).warps());
    assert!(MaskFrame::downstream(&inert, 0.0).unwarp((240.0, 160.0)).is_none());
    assert!(!MaskFrame::downstream(&toggled_off.lens_profile, 0.0).warps());
    // …and an ACTIVE one answers the other way, in both of the two ways it
    // can be active (profile knots, and the manual amount alone).
    assert!(MaskFrame::downstream(&barrel_profile(), 0.0).warps());
    assert!(MaskFrame::downstream(&inert, 25.0).warps(), "the manual amount alone warps");
    assert!(
        MaskFrame::downstream(&inert, 25.0).unwarp((240.0, 160.0)).is_some(),
        "the manual lens_distortion must be covered, not half-covered"
    );
}

/// R38: the fit's rulers judge `develop_preview`'s pixels, so the contour
/// they build must be the one that develop evaluates — the clamped
/// recipe's composed profile and manual amount, exactly as
/// `develop_preview_inner` decides them. Under a barrel profile the
/// stored-frame contour of a linear edge is not where the preview paints
/// it (LINEAR samples through `MaskUnwarp::engine_at`); with no geometry
/// to follow the two frames are one.
#[test]
fn preview_mask_coverage_is_the_frame_the_preview_develops_in() {
    let (w, h) = (480u32, 320u32);
    let base = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([120, 120, 120])));
    // The barrel of `a_parametric_mask_lands_on_its_stored_coordinates_under_lens_geometry`
    // (corner factor 0.75, fill scale 0.923): its composite map magnifies
    // the frame's middle by 8 %, so a hard edge 72 px left of centre paints
    // 2–5 px away from its stored column, farthest on the centre row.
    // `barrel_profile()` moves it under a pixel at this size.
    let profile = crate::recipe::LensProfile {
        distortion: (0..16).map(|i| 1.0 - 0.25 * (i as f32 / 15.0).powi(2)).collect(),
        distortion_on: true,
        ..Default::default()
    };
    let edge = crate::recipe::LocalAdjustment {
        mask: MaskGeometry::Linear { zero_x: 0.35 + 0.5 / w as f32, zero_y: 0.5, full_x: 0.35, full_y: 0.5 },
        exposure_ev: 0.6,
        ..Default::default()
    };
    let recipe = EditRecipe { lens_profile: profile.clone(), masks: vec![edge.clone()], ..Default::default() };
    let validated = crate::recipe::ValidatedRecipe::new(&recipe);
    let expected = mask_coverage(
        &edge,
        &base,
        MaskFrame::downstream(&geometry_profile(&validated), validated.lens_distortion),
    );
    let got = preview_mask_coverage(&edge, &base, &recipe);
    assert_eq!(got.as_raw(), expected.as_raw(), "the preview's own frame, from the recipe");
    let stored = mask_coverage(&edge, &base, MaskFrame::AsRendered);
    let rendered = develop_preview(&base, &recipe).to_rgb8();
    let plain = develop_preview(&base, &EditRecipe { lens_profile: profile, ..Default::default() }).to_rgb8();
    let lift = |x: u32, y: u32| rendered.get_pixel(x, y)[1] as i32 - plain.get_pixel(x, y)[1] as i32;
    let full = lift(8, 160);
    assert!(full > 20, "premise: the edge lifts its side by {full} codes");
    for y in [60u32, 160, 260] {
        let painted = (0..w).rev().find(|&x| lift(x, y) * 2 > full).expect("a lifted pixel");
        let contour = |cov: &image::GrayImage| (0..w).rev().find(|&x| cov.get_pixel(x, y)[0] >= 128).expect("coverage");
        assert!(
            contour(&stored).abs_diff(painted) >= 2,
            "row {y}: premise — the barrel paints the edge at {painted}, off the stored column {}",
            contour(&stored)
        );
        assert!(
            contour(&got).abs_diff(painted) <= 1,
            "row {y}: the preview-frame contour ({}) is where the develop paints the edge ({painted})",
            contour(&got)
        );
    }
    let flat = EditRecipe { masks: vec![edge.clone()], ..Default::default() };
    assert_eq!(preview_mask_coverage(&edge, &base, &flat).as_raw(), stored.as_raw(), "no geometry: one frame");
}

/// The GUI's red coverage wash must advertise exactly what the render
/// applies — including the frame adaptation.
///
/// The overlay is built by `mask_coverage` and then warped by the caller
/// (`bin/gui/canvas.rs`) so it follows the rendered pixels. Both halves take
/// the SAME [`MaskFrame`]; if the coverage build skipped the adaptation
/// while the render performed it, the wash would sit a whole field away
/// from the effect it claims to show — up to 186 px at 24 mm.
///
/// Asserted as agreement between the two, not against a hand-computed
/// expectation: the property is that the overlay and the render say the
/// same thing, whatever that thing is.
///
/// MUTATION THIS KILLS: dropping the `unwarp` from `mask_coverage`.
#[test]
fn the_gui_coverage_overlay_matches_what_the_render_applies() {
    let (w, h) = (480u32, 320u32);
    let base = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb([255, 255, 255])));
    let profile = crate::recipe::LensProfile {
        distortion: (0..16).map(|i| 1.0 - 0.25 * (i as f32 / 15.0).powi(2)).collect(),
        distortion_on: true,
        ..Default::default()
    };
    let adj = probe_radial(0.20, 0.5, 0.08);
    let recipe =
        EditRecipe { masks: vec![adj.clone()], lens_profile: profile.clone(), ..Default::default() };
    let frame = MaskFrame::downstream(&profile, 0.0);
    assert!(frame.warps(), "premise: this fixture's geometry is active");

    // The overlay, exactly as the GUI builds it: coverage then the same warp.
    let cov = DynamicImage::ImageLuma8(mask_coverage(&adj, &base, frame));
    let cov = apply_lens_geometry(&cov, &profile, 0.0).to_luma8();

    // The render, exactly as a preview surface builds it.
    let rendered = apply_lens_geometry(&develop_preview(&base, &recipe), &profile, 0.0).to_rgb8();

    // Where the overlay claims full coverage, the render must have applied
    // the effect (-4 EV on white); where it claims none, it must not have.
    let (mut claimed, mut agreed, mut clear, mut clean) = (0u32, 0u32, 0u32, 0u32);
    for (x, y, p) in cov.enumerate_pixels() {
        let lit = rendered.get_pixel(x, y).0[1];
        if p.0[0] > 200 {
            claimed += 1;
            if lit < 160 {
                agreed += 1;
            }
        } else if p.0[0] < 20 {
            clear += 1;
            if lit > 200 {
                clean += 1;
            }
        }
    }
    assert!(claimed > 200 && clear > 200, "premise: {claimed} covered / {clear} clear px");
    assert!(
        agreed * 100 >= claimed * 97,
        "the overlay claims coverage the render does not apply: {agreed}/{claimed}"
    );
    assert!(
        clean * 100 >= clear * 97,
        "the render applies an effect the overlay does not show: {clean}/{clear}"
    );
}

/// The per-TYPE split, at the predicate that owns it: only the two shapes
/// Lightroom stores post-correction are adapted.
///
/// A brush would be moved TWICE if it were included here (Lightroom
/// rasterises it pre-correction and so does this engine); an engine-authored
/// raster has no Lightroom rendering to agree with; a range mask selects by
/// pixel value and is frame-invariant, so it is not a geometry at all.
#[test]
fn only_lightrooms_post_correction_shapes_are_frame_adapted() {
    assert!(is_lr_post_correction_geometry(&MaskGeometry::Linear {
        zero_x: 0.1,
        zero_y: 0.2,
        full_x: 0.8,
        full_y: 0.9
    }));
    assert!(is_lr_post_correction_geometry(&probe_radial(0.3, 0.3, 0.1).mask));
    assert!(!is_lr_post_correction_geometry(&MaskGeometry::Brush {
        name: "Brush 1".into(),
        blend_mode: 0,
        value: 1.0,
        inverted: false,
        strokes: Vec::new(),
    }));
    assert!(!is_lr_post_correction_geometry(&MaskGeometry::Bitmap { path: String::new() }));
    assert!(!is_lr_post_correction_geometry(&MaskGeometry::AiMask {
        name: String::new(),
        subtype: 0,
        ref_x: 0.5,
        ref_y: 0.5,
        blend_mode: 0,
        value: 1.0,
        inverted: false,
        mask_version: 0,
        gesture: Vec::new(),
        provenance: Vec::new(),
        raster: None,
    }));
    // And the adapter honours it: with the SAME unwarp in hand, a brush
    // and a bitmap are asked at the un-adapted point.
    let u = MaskUnwarp::new(&barrel_profile(), 0.0, (480.0, 320.0)).expect("active");
    let moved = u.at(0.2, 0.5);
    assert!((moved.0 - 0.2).abs() > 1e-4, "premise: the map really moves this point");
    // A brush that actually PAINTS at the sample point (R29 Batch-6b). With
    // the empty stroke list this test used to carry, both sides of the
    // equality were the inert 0 and it held for the wrong reason. The
    // sample sits on the RIM of a hard dab (h = 1, so the falloff is a
    // near-step) — the one place a fraction of a per-cent of frame width
    // changes the weight by more than the 8-bit raster quantum.
    let brush = probe_brush(&[(1.0, 0.05, 1.0, 1.0, "d 0.2 0.5")]);
    let braster = brush_raster(&brush, 480, 320).expect("one dab");
    let rim = (0.2 + 0.05 * 0.94, 0.5);
    let rim_moved = u.at(rim.0, rim.1);
    assert!(
        mask_weight(&brush, rim.0, rim.1, Some(&braster)) > 0.2,
        "premise: the brush really paints at the rim sample"
    );
    assert_eq!(
        mask_weight_in(&brush, rim.0, rim.1, Some(&braster), Some(&u), (480.0, 320.0)),
        mask_weight(&brush, rim.0, rim.1, Some(&braster)),
        "a brush must not be frame-adapted"
    );
    assert!(
        (mask_weight_in(&brush, rim.0, rim.1, Some(&braster), Some(&u), (480.0, 320.0))
            - mask_weight(&brush, rim_moved.0, rim_moved.1, Some(&braster)))
        .abs()
            > 1e-3,
        "…and the adapted point is a DIFFERENT weight, so the equality is not vacuous"
    );
    let rad = probe_radial(0.24, 0.5, 0.1).mask;
    assert_eq!(
        mask_weight_in(&rad, 0.2, 0.5, None, Some(&u), (480.0, 320.0)),
        mask_weight(&rad, moved.0, moved.1, None),
        "a radial must be asked at the adapted point"
    );
}

/// The frame table in the mask-warp block header, asserted rather than
/// asserted-in-prose: this engine evaluates masks BEFORE the geometry
/// stage, so a mask it draws is carried by the distortion field exactly as
/// the pixels are.
///
/// That is what makes an extra warp on the brush arm a DOUBLE application
/// (Lightroom rasterises brush dabs pre-correction too), and what makes the
/// radial arm — whose coordinates Lightroom stores POST-correction — a
/// mismatch this batch exposed rather than created.
///
/// Measured here rather than cited, in the two halves that make an order:
/// the mask stage's OUTPUT does not depend on the lens profile at all (it
/// runs first and cannot see it), and running the geometry stage over that
/// output MOVES the mask's footprint (so it runs second, over pixels the
/// mask is already baked into). That composition — `apply_develop` then
/// `apply_lens_geometry` — is exactly the one `render_to_file` performs.
#[test]
fn the_engine_evaluates_masks_before_the_geometry_stage() {
    // 480 px wide, not 240: the displacement this measures is a FRACTION of
    // the frame (~0.6 % of the width for this profile), so a small fixture
    // frame puts the whole effect inside a pixel and the test would pass on
    // rounding rather than on the property.
    let base = DynamicImage::ImageRgb8(RgbImage::from_pixel(480, 320, image::Rgb([128, 128, 128])));
    let mask = crate::recipe::LocalAdjustment {
        mask: MaskGeometry::Radial {
            top: 0.30,
            left: 0.05,
            bottom: 0.70,
            right: 0.28,
            feather: 0.0,
            roundness: 0.0,
            flipped: false,
            angle: 0.0,
            midpoint: 50.0,
            mask_version: 0,
        },
        exposure_ev: -4.0,
        ..Default::default()
    };
    // A strong barrel profile, shaped like a real one (falling factors).
    let profile = crate::recipe::LensProfile {
        distortion: (0..16).map(|i| 1.0 - 0.12 * (i as f32 / 15.0).powi(2)).collect(),
        distortion_on: true,
        ..Default::default()
    };
    let off = EditRecipe { masks: vec![mask.clone()], ..Default::default() };
    let on = EditRecipe { lens_profile: profile.clone(), ..off.clone() };
    let dark_centroid = |img: &DynamicImage| -> (f64, f64) {
        let g = img.to_rgb8();
        let (mut sx, mut sy, mut n) = (0.0f64, 0.0f64, 0.0f64);
        for (x, y, p) in g.enumerate_pixels() {
            if p.0[1] < 90 {
                sx += x as f64;
                sy += y as f64;
                n += 1.0;
            }
        }
        assert!(n > 50.0, "the mask must darken a real region, got {n} px");
        (sx / n, sy / n)
    };
    // HALF ONE: the mask stage runs FIRST — it rasterises into the
    // PRE-geometry buffer and nothing it does moves a pixel. Asked with
    // `AsRendered` (no geometry downstream) it renders the same pixels
    // whether or not a profile is present, because the profile is not a
    // tonal control and the frame adaptation is switched off.
    //
    // Deliberately NOT `develop_preview` here any more: since R29 Batch-3
    // that entry point DOES read the profile — to adapt a parametric mask's
    // frame for the resample it expects to follow (`MaskFrame`). That is
    // the fix, not a counter-example to the ordering, and asking with the
    // frame pinned is how the ordering stays measurable.
    let anon = crate::diag::pixels();
    let masked_off = develop_preview_framed(&base, &off, &anon, MaskFrame::AsRendered, None, false);
    let masked_on = develop_preview_framed(&base, &on, &anon, MaskFrame::AsRendered, None, false);
    assert_eq!(
        masked_off.to_rgb8().as_raw(),
        masked_on.to_rgb8().as_raw(),
        "the mask stage moved pixels by itself - it is not a pure rasteriser"
    );
    // HALF TWO: the geometry stage runs SECOND, over pixels the mask is
    // already baked into, so it CARRIES the mask exactly as it carries the
    // photograph. If masks were evaluated after geometry this would be
    // zero, and the brush arm would need the warp the block header
    // describes instead of already having it.
    let before = dark_centroid(&masked_off);
    let after = dark_centroid(&apply_lens_geometry(&masked_off, &profile, 0.0));
    assert!(
        (before.0 - after.0).abs() > 1.5,
        "the geometry stage did not carry the mask: centroids {before:?} vs {after:?}"
    );
}

#[test]
fn apply_lens_distortion_fills_the_frame_and_moves_content_radially() {
    // (a) 0 is the identity (pixels untouched); dims always preserved.
    let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(121, 81, image::Rgb([9, 200, 30])));
    assert_eq!(apply_lens_distortion(&img, 0.0).to_rgb8().as_raw(), img.to_rgb8().as_raw());
    let out = apply_lens_distortion(&img, 70.0);
    assert_eq!((out.width(), out.height()), (121, 81));

    // (b) No un-sampled (black) pixels for EITHER sign: k ≤ 0 fills by
    // construction, k > 0 relies on the Newton fill scale.
    let white = DynamicImage::ImageRgb8(RgbImage::from_pixel(121, 81, image::Rgb([255, 255, 255])));
    for amt in [100.0f32, -100.0, 55.0, -55.0] {
        let r = apply_lens_distortion(&white, amt).to_rgb16();
        let min = r.pixels().flat_map(|p| p.0).min().unwrap();
        assert!(min >= 65000, "unfilled pixels at amount {amt}: min {min}");
    }

    // (c) The exact centre is a fixed point of the resample.
    let mut cdot = RgbImage::from_pixel(121, 81, image::Rgb([0, 0, 0]));
    cdot.put_pixel(60, 40, image::Rgb([255, 255, 255]));
    for amt in [100.0f32, -100.0] {
        let m = apply_lens_distortion(&DynamicImage::ImageRgb8(cdot.clone()), amt).to_rgb16();
        assert!(m.get_pixel(60, 40)[0] > 30000, "centre must be a fixed point at {amt}");
    }

    // (d) A +100 barrel fix (fill scale = 1) pushes content OUTWARD: a
    // white 3×3 dot centred at x=30 on the horizontal centreline (frame
    // centre x=60) must land further LEFT (predicted ≈ x 28.6).
    let mut dot = RgbImage::from_pixel(121, 81, image::Rgb([0, 0, 0]));
    for yy in 39..=41 {
        for xx in 29..=31 {
            dot.put_pixel(xx, yy, image::Rgb([255, 255, 255]));
        }
    }
    let moved = apply_lens_distortion(&DynamicImage::ImageRgb8(dot), 100.0).to_rgb16();
    let bright_x = (0..121u32).max_by_key(|&x| moved.get_pixel(x, 40)[0]).unwrap();
    assert!(
        moved.get_pixel(bright_x, 40)[0] > 30000 && bright_x <= 29,
        "barrel fix must move the dot outward (x<30), got x={bright_x}"
    );
}

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

#[test]
fn orient_f32_matches_the_display_orientation_semantics() {
    // A 3×2 buffer whose pixels carry their own index: every one of the
    // EIGHT states must produce the exact hand-derived EXIF mapping
    // (Rotate90 = clockwise, so top-left → top-right; Transpose =
    // main-diagonal mirror; Transverse = anti-diagonal). Only Normal and
    // Rotate90 were pinned before — the other six arms of `oriented`,
    // including the two in-place A7 compositions, had no coverage, and
    // orient_f32 round-trips through `oriented`, so the table below is
    // derived from the EXIF definitions, NOT from the image crate (a
    // crate-op reference would compare the function against itself)
    // (U14).
    let src: Vec<[f32; 3]> = (0..6).map(|i| [i as f32, 0.0, 0.0]).collect();
    let cases: [(Orientation, (usize, usize), [usize; 6]); 8] = [
        (Orientation::Normal, (3, 2), [0, 1, 2, 3, 4, 5]),
        (Orientation::HorizontalFlip, (3, 2), [2, 1, 0, 5, 4, 3]),
        (Orientation::Rotate180, (3, 2), [5, 4, 3, 2, 1, 0]),
        (Orientation::VerticalFlip, (3, 2), [3, 4, 5, 0, 1, 2]),
        (Orientation::Rotate90, (2, 3), [3, 0, 4, 1, 5, 2]),
        (Orientation::Rotate270, (2, 3), [2, 5, 1, 4, 0, 3]),
        (Orientation::Transpose, (2, 3), [0, 3, 1, 4, 2, 5]),
        (Orientation::Transverse, (2, 3), [5, 2, 4, 1, 3, 0]),
    ];
    for (o, dims, map) in cases {
        let (out, w, h) = orient_f32(src.clone(), 3, 2, o);
        assert_eq!((w, h), dims, "{o:?} dims");
        let got: Vec<usize> = out.iter().map(|p| p[0] as usize).collect();
        assert_eq!(&got[..], &map[..], "{o:?} pixel mapping");
    }
}

/// THE SEAM the whole coordinate migration rests on: `orient_point` must
/// be the exact coordinate twin of `oriented`'s PIXEL transform, for all
/// eight states. Derived from the pixel map above (which is itself derived
/// from the EXIF definitions, never from the image crate), so a future
/// edit to either function that drifts from the other fails HERE rather
/// than silently displacing every saved mask.
#[test]
fn orient_point_is_the_coordinate_twin_of_the_pixel_transform() {
    const W: u32 = 5;
    const H: u32 = 3;
    let src = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(W, H, |x, y| {
        image::Rgb([x as u8, y as u8, 0])
    }));
    for o in [
        Orientation::Normal,
        Orientation::HorizontalFlip,
        Orientation::Rotate180,
        Orientation::VerticalFlip,
        Orientation::Transpose,
        Orientation::Rotate90,
        Orientation::Transverse,
        Orientation::Rotate270,
    ] {
        let dst = oriented(src.clone(), o).to_rgb8();
        let (dw, dh) = (dst.width(), dst.height());
        for y in 0..H {
            for x in 0..W {
                // Pixel CENTRES: the only points whose normalised image is
                // unambiguous under a bin edge.
                let (u, v) =
                    orient_point(o, (x as f32 + 0.5) / W as f32, (y as f32 + 0.5) / H as f32);
                let (dx, dy) = ((u * dw as f32) as u32, (v * dh as f32) as u32);
                let px = dst.get_pixel(dx.min(dw - 1), dy.min(dh - 1));
                assert_eq!(
                    (px[0] as u32, px[1] as u32),
                    (x, y),
                    "{o:?}: source ({x},{y}) should land at ({dx},{dy})"
                );
            }
        }
    }
}

/// The nine `Orientation` values under `to_flips`/`from_flips` and the nine
/// under [`orient_point`] are the SAME group, so [`compose_orientation`]'s
/// bit algebra and the geometry it claims to describe cannot drift apart.
///
/// Checked exhaustively: for every (EXIF state, quarter turn) pair and
/// four probe points, `orient_point(compose(e, k), p)` equals
/// `orient_point(R90^k, orient_point(e, p))` — composition IS "apply the
/// EXIF state, then turn the display frame", which is the order the pixels
/// take. The probe set includes an off-frame point, because mask gradients
/// legitimately live outside the unit square.
///
/// MUTATION THIS CATCHES: swap the two arms of `compose_two`'s
/// `if t1 { … } else { … }` (i.e. cross the flips on the wrong side) and
/// every transposing EXIF state composes to its mirror — the 「竖图横躺」
/// failure, one composition step upstream of where it used to live.
#[test]
fn compose_orientation_is_the_composition_of_the_two_coordinate_maps() {
    const STATES: [Orientation; 9] = [
        Orientation::Normal,
        Orientation::HorizontalFlip,
        Orientation::Rotate180,
        Orientation::VerticalFlip,
        Orientation::Transpose,
        Orientation::Rotate90,
        Orientation::Transverse,
        Orientation::Rotate270,
        Orientation::Unknown,
    ];
    for e in STATES {
        for k in 0u8..4 {
            let composed = compose_orientation(e, k);
            // The group is CLOSED: nine values in, never `Unknown` out
            // (it is Normal's twin, and a composition that produced it
            // would make `to_u16` write 0 into a sidecar one day).
            assert_ne!(composed, Orientation::Unknown, "{e:?} + {k} quarter turns");
            for (u, v) in [(0.0f32, 0.0f32), (0.13, 0.87), (1.0, 0.25), (-0.4, 1.6)] {
                let (a, b) = orient_point(e, u, v);
                let want = orient_point(quarter_turn_orientation(k), a, b);
                let got = orient_point(composed, u, v);
                assert!(
                    (got.0 - want.0).abs() < 1e-6 && (got.1 - want.1).abs() < 1e-6,
                    "{e:?} then {k} quarter turns = {composed:?}: ({u},{v}) → {got:?}, \
                     but applying the two in order gives {want:?}"
                );
            }
        }
    }
    // …and the identity/period facts the field's 0..3 domain rests on.
    assert_eq!(compose_orientation(Orientation::Rotate90, 0), Orientation::Rotate90);
    assert_eq!(compose_orientation(Orientation::Rotate90, 2), Orientation::Rotate270);
    assert_eq!(compose_orientation(Orientation::Rotate270, 1), Orientation::Normal);
    assert_eq!(compose_orientation(Orientation::Normal, 4), Orientation::Normal);
}

/// [`quarter_turns_between`] IS [`compose_orientation`]'s inverse, over the
/// whole 8 x 8 table of (capture EXIF state, sidecar `tiff:Orientation`)
/// pairs — the sidecar orientation law, checked rather than argued.
///
/// Three properties, exhaustively:
///   * every answer it gives is right: `compose(exif, k) == want`;
///   * it answers exactly when the two states have the same HANDEDNESS,
///     which is the index-two rotation subgroup fact the doc comment rests
///     on — so a refusal is a real impossibility, not a search that gave up;
///   * the me6-2026-09 pack's own row: EXIF 8 over a sidecar declaring 1 is
///     one clockwise quarter turn, which is what makes the delivered frame
///     6240 x 4160 instead of 4160 x 6240.
///
/// MUTATION THIS CATCHES: search `0u8..3` instead of `0u8..4` and the 16
/// pairs needing three quarter turns come back `None` — a refused rotation
/// reported as a mirror.
#[test]
fn quarter_turns_between_is_the_inverse_of_compose_orientation() {
    const STATES: [Orientation; 8] = [
        Orientation::Normal,
        Orientation::HorizontalFlip,
        Orientation::Rotate180,
        Orientation::VerticalFlip,
        Orientation::Transpose,
        Orientation::Rotate90,
        Orientation::Transverse,
        Orientation::Rotate270,
    ];
    let mut solved = 0;
    for exif in STATES {
        for want in STATES {
            let reachable = orientation_mirrors(exif) == orientation_mirrors(want);
            match quarter_turns_between(exif, want) {
                Some(k) => {
                    assert!(k < 4, "{exif:?} -> {want:?}: {k} is not a quarter turn");
                    assert_eq!(
                        compose_orientation(exif, k),
                        want,
                        "{exif:?} + {k} quarter turns is not {want:?}"
                    );
                    assert!(reachable, "{exif:?} -> {want:?} crosses a mirror and cannot be a turn");
                    solved += 1;
                }
                None => assert!(
                    !reachable,
                    "{exif:?} -> {want:?} keeps its handedness, so some quarter turn reaches it"
                ),
            }
        }
    }
    // Half the table is reachable, which IS the index-two statement.
    assert_eq!(solved, 32, "the rotation subgroup has index two in the dihedral group");
    // `Unknown` is `Normal`'s twin on both sides, never a refusal.
    assert_eq!(quarter_turns_between(Orientation::Unknown, Orientation::Normal), Some(0));
    assert_eq!(quarter_turns_between(Orientation::Normal, Orientation::Unknown), Some(0));
    // The pack: IFD0 Orientation = 8 under a sidecar that declares 1.
    assert_eq!(
        quarter_turns_between(Orientation::Rotate270, Orientation::Normal),
        Some(1),
        "the me6-2026-09 pack's own row"
    );
}

/// The 竖图横躺 regression net (R27 A10), stated as the property that
/// actually matters: the composed orientation TRANSPOSES exactly when the
/// rendered frame does.
///
/// Pure code — the pixel side is [`oriented`] on a deliberately
/// non-square frame, which is the same function `orient_f32` round-trips
/// through, so no RAW is needed to pin the chain. The real-file arm lives
/// in `decode::portrait_raw_reaches_the_pipeline_as_rotate270` and the RAW
/// zoo probe.
///
/// MUTATION THIS CATCHES: drop `Rotate270` from `decode`'s
/// `orientation_transposes` list (or add `Rotate180` to it) and the
/// declared dims part company with the pixels for half the states — the
/// exact shape of the v0.30 root fix, now with the user's turn on top.
#[test]
fn a_quarter_turn_on_any_exif_state_transposes_the_dims_iff_it_transposes_the_pixels() {
    const STATES: [Orientation; 8] = [
        Orientation::Normal,
        Orientation::HorizontalFlip,
        Orientation::Rotate180,
        Orientation::VerticalFlip,
        Orientation::Transpose,
        Orientation::Rotate90,
        Orientation::Transverse,
        Orientation::Rotate270,
    ];
    // 7×5: both dims distinct AND distinct from each other's, so a
    // transpose is visible and a square frame cannot hide a bug.
    let src = DynamicImage::ImageRgb8(RgbImage::new(7, 5));
    for e in STATES {
        for k in 0u8..4 {
            let composed = compose_orientation(e, k);
            let (w, h) = oriented(src.clone(), composed).dimensions();
            let transposed = (w, h) == (5, 7);
            assert!(
                transposed || (w, h) == (7, 5),
                "{e:?} + {k}: a rotation/flip produced {w}×{h} from 7×5"
            );
            assert_eq!(
                transposed,
                crate::decode::orientation_transposes(composed),
                "{e:?} + {k} = {composed:?}: pixels {w}×{h} but the dims predicate disagrees"
            );
            // The user's turn applied to the ALREADY-oriented pixels must
            // give the same frame as the composed one — the property that
            // lets `render_to_image_in` keep exactly one orientation stage.
            let two_step = turn_image(oriented(src.clone(), e), k);
            assert_eq!(
                two_step.dimensions(),
                (w, h),
                "{e:?} + {k}: one composed turn and two sequential turns disagree"
            );
        }
    }
}

/// Every state is a BIJECTION of the plane, and the migration is therefore
/// reversible — the property that lets an era-0 recipe be turned exactly
/// once with no accumulated drift.
#[test]
fn orient_point_round_trips_through_its_inverse() {
    // Six of the eight are involutions; the quarter turns are each
    // other's inverse.
    let pairs = [
        (Orientation::Normal, Orientation::Normal),
        (Orientation::Unknown, Orientation::Unknown),
        (Orientation::HorizontalFlip, Orientation::HorizontalFlip),
        (Orientation::VerticalFlip, Orientation::VerticalFlip),
        (Orientation::Rotate180, Orientation::Rotate180),
        (Orientation::Transpose, Orientation::Transpose),
        (Orientation::Transverse, Orientation::Transverse),
        (Orientation::Rotate90, Orientation::Rotate270),
        (Orientation::Rotate270, Orientation::Rotate90),
    ];
    for (o, inv) in pairs {
        // Off-frame points included: mask gradients legitimately live
        // outside [0,1] and must survive the round trip too.
        for (u, v) in [(0.0f32, 0.0f32), (0.13, 0.87), (1.0, 0.0), (-0.4, 1.6)] {
            let (a, b) = orient_point(o, u, v);
            let back = orient_point(inv, a, b);
            assert!(
                (back.0 - u).abs() < 1e-6 && (back.1 - v).abs() < 1e-6,
                "{o:?} then {inv:?} moved ({u},{v}) to {back:?}"
            );
        }
    }
}

/// The A7R IV's own frame — `DefaultCropSize = (9504, 6336)`, aspect
/// exactly 1.5 — as `orient_recipe_coords`' third argument.
///
/// A round number on purpose: `1.5` and its reciprocal are exact in binary,
/// so a radius that survives a four-turn circle in these tests survives it
/// because the algebra is right, not because the aspect happened to cancel
/// its own rounding.
fn probe_frame() -> Option<CoordFrame> {
    CoordFrame::new(9504.0, 6336.0)
}

/// The `angle` half of the radial rule, checked against `mask_weight`
/// itself rather than against the derivation: a turned ELLIPSE must cover
/// exactly the turned PIXELS. This is what makes "rotate the two corners,
/// negate the angle only for mirrors" more than an assertion.
#[test]
fn rotated_radial_mask_covers_the_rotated_pixels() {
    use crate::recipe::LocalAdjustment;
    let base = MaskGeometry::Radial {
        top: 0.15,
        left: 0.30,
        bottom: 0.55,
        right: 0.90,
        feather: 0.4,
        roundness: 0.0,
        flipped: false,
        angle: 37.0,
        midpoint: 50.0,
        mask_version: 2,
    };
    for o in [
        Orientation::HorizontalFlip,
        Orientation::Rotate180,
        Orientation::VerticalFlip,
        Orientation::Transpose,
        Orientation::Rotate90,
        Orientation::Transverse,
        Orientation::Rotate270,
    ] {
        let mut r = EditRecipe {
            masks: vec![LocalAdjustment { mask: base.clone(), ..Default::default() }],
            ..Default::default()
        };
        assert!(orient_recipe_coords(&mut r, o, probe_frame()));
        let turned = &r.masks[0].mask;
        for i in 0..=20 {
            for j in 0..=20 {
                let (u, v) = (i as f32 / 20.0, j as f32 / 20.0);
                let (u2, v2) = orient_point(o, u, v);
                let before = mask_weight(&base, u, v, None);
                let after = mask_weight(turned, u2, v2, None);
                assert!(
                    (before - after).abs() < 1e-4,
                    "{o:?}: weight at ({u},{v}) was {before}, at the turned point ({u2},{v2}) it is {after}"
                );
            }
        }
    }
}

/// The recipe-level migration: crop and every parametric geometry move,
/// the round trip is exact, and a Normal photo is untouched.
/// R33 §G. The field is a RECIPE control now, so the engine has to render
/// it — after the masks, on the frame the masks produced, and only when it
/// is renderable at all.
///
/// The "after the masks" half is the one worth a test: the field's guide
/// is the render's own smoothed luma, so applying it before a mask that
/// moves luma would read a different bin out of the grid's eight and
/// deliver a different correction. The assertion is therefore not "the
/// pixels moved" but "they moved by exactly what `apply_colour_field` does
/// to the MASKED render" — within the one code of slack a second u8
/// quantisation costs.
#[test]
fn the_engine_renders_the_colour_field_after_the_masks_it_reads() {
    use crate::fit::pixels_of;
    use crate::recipe::{ColourField, LocalAdjustment, MaskGeometry};
    let (w, h) = (96u32, 64u32);
    let img = DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
        let v = 0.15 + 0.7 * (x as f32 / (w - 1) as f32) + 0.1 * (y as f32 / (h - 1) as f32);
        let c = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        image::Rgb([c, c, (c as f32 * 0.92) as u8])
    }));
    // A field with a real, spatially varying demand: warmer on the left,
    // cooler on the right, and a different EV per luma bin.
    let (fx, fy, fb) = (4usize, 3usize, 4usize);
    let grid: Vec<[f32; 5]> = (0..fx * fy * fb)
        .map(|v| {
            let (cell, bin) = (v / fb, v % fb);
            let x = (cell % fx) as f32 / (fx - 1) as f32;
            [0.10 * (bin as f32 / (fb - 1) as f32) - 0.05, 0.06 * x, 0.0, -0.06 * x, 0.02]
        })
        .collect();
    let field = ColourField { x: fx, y: fy, b: fb, grid, amount: 1.0, enabled: true };

    let with_mask = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Linear { zero_x: 0.0, zero_y: 0.0, full_x: 1.0, full_y: 0.0 },
            exposure_ev: -0.9,
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut both = with_mask.clone();
    both.colour_field = Some(field.clone());

    let masked = pixels_of(&develop_preview(&img, &with_mask));
    let mut expected = masked.clone();
    apply_colour_field(&mut expected, w as usize, h as usize, Some(&field));
    let actual = pixels_of(&develop_preview(&img, &both));
    assert_ne!(actual, masked, "premise: this field is not the identity here");
    let worst = actual
        .iter()
        .zip(&expected)
        .flat_map(|(a, b)| (0..3).map(move |c| (a[c] - b[c]).abs()))
        .fold(0.0f32, f32::max);
    assert!(
        worst <= 1.5 / 255.0,
        "the engine's field must be apply_colour_field over the MASKED render; worst {}/255",
        worst * 255.0
    );

    // …and every way of saying "not now" leaves the frame exactly as the
    // masks left it. A mis-shaped grid is in this list on purpose: it is
    // not a field to be guessed at.
    for parked in [
        ColourField { enabled: false, ..field.clone() },
        ColourField { amount: 0.0, ..field.clone() },
        ColourField { x: fx + 1, ..field.clone() },
        ColourField { grid: Vec::new(), ..field.clone() },
    ] {
        let mut off = with_mask.clone();
        off.colour_field = Some(parked);
        assert_eq!(
            pixels_of(&develop_preview(&img, &off)),
            masked,
            "a parked or mis-shaped field must leave the frame untouched"
        );
    }
}

/// R33 §G. A quarter turn moves the field's cells with every other
/// coordinate in the recipe, and swaps its two axis LENGTHS with them.
///
/// Round-tripped through the inverse orientation, because a permutation
/// that is off by one cell still looks plausible in isolation: only going
/// back proves each cell landed where exactly one cell came from.
#[test]
fn a_quarter_turn_transposes_the_colour_field_and_comes_back() {
    use crate::recipe::ColourField;
    let (fx, fy, fb) = (4usize, 3usize, 2usize);
    let seed = |i: usize| [i as f32, 0.0, 0.0, 0.0, 0.0];
    let field = ColourField {
        x: fx,
        y: fy,
        b: fb,
        grid: (0..fx * fy * fb).map(seed).collect(),
        amount: 1.0,
        enabled: true,
    };
    for (there, back) in [
        (Orientation::Rotate90, Orientation::Rotate270),
        (Orientation::Rotate270, Orientation::Rotate90),
        (Orientation::Transpose, Orientation::Transpose),
        (Orientation::Rotate180, Orientation::Rotate180),
    ] {
        let mut r = EditRecipe { colour_field: Some(field.clone()), ..Default::default() };
        orient_recipe_coords(&mut r, there, None);
        let turned = r.colour_field.clone().expect("the field survives the turn");
        let swaps = crate::decode::orientation_transposes(there);
        assert_eq!(
            (turned.x, turned.y),
            if swaps { (fy, fx) } else { (fx, fy) },
            "{there:?}: the axis lengths follow the frame"
        );
        assert_eq!(turned.grid.len(), fx * fy * fb, "no vertex was lost or invented");
        orient_recipe_coords(&mut r, back, None);
        assert_eq!(
            r.colour_field.as_ref().expect("still there"),
            &field,
            "{there:?} then {back:?} must be the identity on the grid"
        );
    }
}

#[test]
fn orient_recipe_coords_moves_geometry_and_round_trips() {
    use crate::recipe::{LocalAdjustment, MaskComponent};
    let seed = || EditRecipe {
        crop: Some(Crop { left: 0.1, top: 0.2, right: 0.8, bottom: 0.9 }),
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Linear {
                zero_x: 0.5,
                zero_y: 0.0,
                full_x: 0.5,
                full_y: 0.45,
            },
            components: vec![MaskComponent {
                geometry: MaskGeometry::Radial {
                    top: 0.1,
                    left: 0.2,
                    bottom: 0.6,
                    right: 0.7,
                    feather: 0.5,
                    roundness: 0.0,
                    flipped: false,
                    angle: 0.0,
                    midpoint: 50.0,
                    mask_version: 2,
                },
                ..Default::default()
            }],
            range: Some(RangeMask::Color {
                r: 0.2,
                g: 0.4,
                b: 0.9,
                amount: 0.5,
                px: 0.25,
                py: 0.75,
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    // Normal / Unknown: not a single field moves, and the caller is told
    // nothing happened.
    for o in [Orientation::Normal, Orientation::Unknown] {
        let mut r = seed();
        assert!(!orient_recipe_coords(&mut r, o, probe_frame()), "{o:?} must report no move");
        assert_eq!(r, seed(), "{o:?} must not touch a single coordinate");
    }
    // The portrait ARW case, and back.
    let mut r = seed();
    assert!(orient_recipe_coords(&mut r, Orientation::Rotate270, probe_frame()));
    assert_ne!(r, seed(), "a quarter turn must actually move the geometry");
    // Hand-derived: Rotate270 maps (u,v) -> (v, 1-u), so the crop's
    // left/right come from top/bottom and its top/bottom from 1-right,
    // 1-left.
    let t = r.crop.expect("crop survives");
    for (got, want, what) in [
        (t.left, 0.2, "left"),
        (t.right, 0.9, "right"),
        (t.top, 1.0 - 0.8, "top"),
        (t.bottom, 1.0 - 0.1, "bottom"),
    ] {
        assert!((got - want).abs() < 1e-6, "crop {what}: {got} != {want} ({t:?})");
    }
    assert!(orient_recipe_coords(&mut r, Orientation::Rotate90, probe_frame()));
    let back = seed();
    let (c, c0) = (r.crop.unwrap(), back.crop.unwrap());
    assert!(
        (c.left - c0.left).abs() < 1e-6
            && (c.top - c0.top).abs() < 1e-6
            && (c.right - c0.right).abs() < 1e-6
            && (c.bottom - c0.bottom).abs() < 1e-6,
        "crop round trip: {c:?} vs {c0:?}"
    );
    let (
        MaskGeometry::Linear { zero_x, zero_y, full_x, full_y },
        MaskGeometry::Linear { zero_x: a, zero_y: b, full_x: cx, full_y: cy },
    ) = (&r.masks[0].mask, &back.masks[0].mask)
    else {
        panic!("linear geometry survives")
    };
    assert!(
        (zero_x - a).abs() < 1e-6
            && (zero_y - b).abs() < 1e-6
            && (full_x - cx).abs() < 1e-6
            && (full_y - cy).abs() < 1e-6,
        "linear round trip"
    );
    let Some(RangeMask::Color { px, py, .. }) = r.masks[0].range else {
        panic!("range survives")
    };
    assert!((px - 0.25).abs() < 1e-6 && (py - 0.75).abs() < 1e-6, "range point round trip");
}

/// R27 L-16c, half one. The `coord_era` migration's crop arm is exact ONLY
/// if the frame the crop is normalised against turns with the frame — and
/// that frame is `inscribed_dims`'s output, not the sensor rectangle,
/// because `render_pipeline` straightens before it crops.
///
/// Swapping `w` and `h` must swap the two answers and change nothing else.
/// The general branch shows it by inspection; this pins the branch
/// boundary too, which is where the `if w >= h` inside the thin case could
/// have made the claim false.
///
/// MUTATION THIS CATCHES: collapse the thin branch's
/// `if w >= h { (x/s, x/c) } else { (x/c, x/s) }` to either arm alone and
/// the sliver rows go red (verified). Note what does NOT catch it, because
/// it looks like it should: SWAPPING the general branch's two expressions
/// keeps the property, since exchanging both the inputs and the outputs of
/// a swap-equivariant pair is still swap-equivariant. This test pins the
/// symmetry, not the formula — `the_straighten_angle_reverses_only_under_
/// a_mirror` and the existing crop round trip pin the values.
#[test]
fn the_straightened_frame_turns_with_the_photo() {
    // Real ARW dims, their transpose, a square, and a sliver.
    for (w, h) in
        [(9504.0f32, 6336.0f32), (6336.0, 9504.0), (4000.0, 4000.0), (100.0, 3000.0)]
    {
        for deg in [0.5f32, 2.5, -2.5, 30.0, -44.0, 44.0, 45.0, -45.0] {
            let (a, b) = inscribed_dims(w, h, deg);
            let (c, d) = inscribed_dims(h, w, deg);
            let close = |x: f32, y: f32| (x - y).abs() <= 1e-3 * x.abs().max(1.0);
            assert!(
                close(a, d) && close(b, c),
                "inscribed_dims({w},{h},{deg}) = ({a},{b}) but ({h},{w}) = ({c},{d}) \
                 — the turned frame must be the turn of the frame"
            );
        }
    }
}

/// R27 L-16c, half two. R24 registered 「`straighten≠0` 时 crop 迁移一阶
/// 近似」 without a code site. The residue is the SIGN: rotations commute
/// with a quarter turn, so the four pure rotations were already exact, but
/// `rot(deg) ∘ mirror == mirror ∘ rot(−deg)`, so a mirrored photo was
/// straightened the wrong way by `2·deg` and every crop coordinate then
/// indexed content that had moved out from under it.
///
/// Both angles the registration is quoted against: a routine horizon
/// (2.5°) and the extreme end of the ±45 clamp (−44°).
///
/// MUTATION THIS CATCHES: delete the `if mirrors { r.straighten_deg = … }`
/// block in `orient_recipe_coords` and the four mirror rows go red; negate
/// on EVERY orientation instead and the three rotation rows go red.
#[test]
fn the_straighten_angle_reverses_only_under_a_mirror() {
    let seed = |deg: f32| EditRecipe {
        straighten_deg: deg,
        crop: Some(Crop { left: 0.1, top: 0.2, right: 0.8, bottom: 0.9 }),
        ..Default::default()
    };
    for deg in [2.5f32, -44.0] {
        // A quarter or half turn carries the tilt unchanged: the content
        // and the frame turned together.
        for o in [Orientation::Rotate90, Orientation::Rotate180, Orientation::Rotate270] {
            let mut r = seed(deg);
            assert!(orient_recipe_coords(&mut r, o, probe_frame()));
            assert_eq!(r.straighten_deg, deg, "{o:?} must not touch the tilt");
        }
        // A reflection reverses it — and these four are involutions, so
        // applying the same one twice is the identity (the round trip the
        // migration's bijectivity claim rests on).
        for o in [
            Orientation::HorizontalFlip,
            Orientation::VerticalFlip,
            Orientation::Transpose,
            Orientation::Transverse,
        ] {
            let mut r = seed(deg);
            assert!(orient_recipe_coords(&mut r, o, probe_frame()));
            assert_eq!(r.straighten_deg, -deg, "{o:?} must reverse the tilt");
            assert!(orient_recipe_coords(&mut r, o, probe_frame()));
            assert_eq!(r.straighten_deg, deg, "{o:?} twice is the identity");
            // …and the crop came home with it (float tolerance, not `==`:
            // `1 − (1 − 0.1)` is 0.10000002 in f32).
            let (c, c0) = (r.crop.unwrap(), seed(deg).crop.unwrap());
            assert!(
                (c.left - c0.left).abs() < 1e-6
                    && (c.top - c0.top).abs() < 1e-6
                    && (c.right - c0.right).abs() < 1e-6
                    && (c.bottom - c0.bottom).abs() < 1e-6,
                "{o:?}: crop round trip {c:?} vs {c0:?}"
            );
        }
    }
    // And a photo with no tilt is untouched whatever the state.
    for o in [Orientation::Transverse, Orientation::Rotate90] {
        let mut r = seed(0.0);
        assert!(orient_recipe_coords(&mut r, o, probe_frame()));
        assert_eq!(r.straighten_deg, 0.0);
    }
}

/// A raster mask is an image FILE: the coordinate rewrites must leave its
/// path alone — its owner re-writes the file and re-points the path
/// afterwards (`pipeline::rotate_recipe`, `pipeline::migrate_raster_file`).
#[test]
fn raster_masks_are_left_for_their_owner_to_rewrite() {
    use crate::recipe::LocalAdjustment;
    let mut r = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Bitmap { path: "sky.png".into() },
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(!recipe_has_frame_coords(&r), "a raster-only recipe has no turnable coordinate");
    let before = r.clone();
    orient_recipe_coords(&mut r, Orientation::Rotate270, probe_frame());
    assert_eq!(r, before, "the raster path must survive byte-for-byte under a turn");
    assert!(shift_recipe_coords(&mut r, 0.01, 0.02));
    assert_eq!(r, before, "and under a translation");
}

/// [`orient_vector`] is exactly the difference of two [`orient_point`]
/// images for every state — the affine map's linear part, spelled per
/// state so a small vector is not read off a subtraction near 1.
#[test]
fn orient_vector_is_the_linear_part_of_orient_point() {
    let states = [
        Orientation::Normal,
        Orientation::Unknown,
        Orientation::HorizontalFlip,
        Orientation::Rotate180,
        Orientation::VerticalFlip,
        Orientation::Transpose,
        Orientation::Rotate90,
        Orientation::Transverse,
        Orientation::Rotate270,
    ];
    let (dx, dy) = (-0.003367f32, 0.003157f32);
    for o in states {
        for (px, py) in [(0.0f32, 0.0f32), (0.3, 0.7), (1.0, 0.25)] {
            let (ax, ay) = orient_point(o, px + dx, py + dy);
            let (bx, by) = orient_point(o, px, py);
            let (vx, vy) = orient_vector(o, dx, dy);
            assert!(
                (ax - bx - vx).abs() < 1e-6 && (ay - by - vy).abs() < 1e-6,
                "{o:?} at ({px}, {py}): ({}, {}) vs ({vx}, {vy})",
                ax - bx,
                ay - by
            );
        }
    }
}

/// The era-2 translation moves every coordinate carrier by the same
/// vector, leaves every length alone, clamps only the crop, and undoes
/// itself.
#[test]
fn shift_recipe_coords_moves_every_carrier_and_round_trips() {
    use crate::recipe::{BrushStroke, ColourField, LocalAdjustment, MaskComponent};
    use crate::retouch::{RetouchArea, RetouchShape};
    let stroke = || BrushStroke {
        radius: 0.05,
        dabs: "r 0.050000\nd 0.500000 0.500000\nf 1.000000\nd 0.600000 0.400000".into(),
        ..Default::default()
    };
    let seed = || EditRecipe {
        crop: Some(Crop { left: 0.0, top: 0.2, right: 0.8, bottom: 1.0 }),
        straighten_deg: 3.0,
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Radial {
                top: 0.1,
                left: 0.2,
                bottom: 0.6,
                right: 0.7,
                feather: 0.5,
                roundness: 0.0,
                flipped: false,
                angle: 15.0,
                midpoint: 50.0,
                mask_version: 2,
            },
            components: vec![
                MaskComponent {
                    geometry: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.0, full_x: 0.5, full_y: 0.45 },
                    ..Default::default()
                },
                MaskComponent {
                    geometry: MaskGeometry::Brush {
                        name: "Brush 1".into(),
                        blend_mode: 0,
                        value: 1.0,
                        inverted: false,
                        strokes: vec![stroke()],
                    },
                    ..Default::default()
                },
                MaskComponent {
                    geometry: MaskGeometry::AiMask {
                        name: "Sky 1".into(),
                        subtype: 2,
                        ref_x: 0.25,
                        ref_y: 0.10,
                        blend_mode: 0,
                        value: 1.0,
                        inverted: false,
                        mask_version: 1,
                        provenance: Vec::new(),
                        gesture: vec![stroke()],
                        raster: Some("alpha.png".into()),
                    },
                    ..Default::default()
                },
            ],
            range: Some(RangeMask::Color { r: 0.2, g: 0.4, b: 0.9, amount: 0.5, px: 0.25, py: 0.75 }),
            ..Default::default()
        }],
        retouch: vec![
            RetouchArea {
                donor: Some([0.3, 0.3]),
                shape: RetouchShape::Ellipse { cx: 0.5, cy: 0.5, size_x: 0.02, size_y: 0.02 },
                ..Default::default()
            },
            RetouchArea { donor: None, shape: RetouchShape::Brush(vec![stroke()]), ..Default::default() },
        ],
        colour_field: Some(ColourField {
            x: 2,
            y: 1,
            b: 1,
            grid: vec![[0.0; 5], [1.0; 5]],
            amount: 1.0,
            enabled: true,
        }),
        ..Default::default()
    };
    let mut r = seed();
    assert!(!shift_recipe_coords(&mut r, 0.0, 0.0), "the zero vector is no move");
    assert_eq!(r, seed());
    let (du, dv) = (0.01f32, -0.02f32);
    assert!(shift_recipe_coords(&mut r, du, dv));
    let near = |a: f32, b: f32| (a - b).abs() < 1e-6;
    let c = r.crop.unwrap();
    assert!(near(c.left, 0.01) && near(c.top, 0.18) && near(c.right, 0.81), "{c:?}");
    assert!(near(c.bottom, 0.98), "bottom moved up, off the edge it sat on: {c:?}");
    assert_eq!(r.straighten_deg, 3.0, "a translation turns nothing");
    let MaskGeometry::Radial { top, left, bottom, right, angle, feather, .. } = r.masks[0].mask else {
        panic!("radial")
    };
    assert!(near(left, 0.21) && near(right, 0.71) && near(top, 0.08) && near(bottom, 0.58));
    assert!(near(right - left, 0.5) && near(bottom - top, 0.5), "the radii are lengths");
    assert_eq!((angle, feather), (15.0, 0.5));
    let MaskGeometry::Linear { zero_x, zero_y, full_x, full_y } = r.masks[0].components[0].geometry else {
        panic!("linear")
    };
    assert!(near(zero_x, 0.51) && near(zero_y, -0.02) && near(full_x, 0.51) && near(full_y, 0.43));
    let MaskGeometry::Brush { strokes, .. } = &r.masks[0].components[1].geometry else { panic!("brush") };
    assert_eq!(strokes[0].dabs, "r 0.050000\nd 0.510000 0.480000\nf 1.000000\nd 0.610000 0.380000");
    assert_eq!(strokes[0].radius, 0.05, "a radius is a length");
    let MaskGeometry::AiMask { ref_x, ref_y, gesture, raster, .. } = &r.masks[0].components[2].geometry
    else {
        panic!("ai mask")
    };
    assert!(near(*ref_x, 0.26) && near(*ref_y, 0.08));
    assert_eq!(gesture[0].dabs, "r 0.050000\nd 0.510000 0.480000\nf 1.000000\nd 0.610000 0.380000");
    assert_eq!(raster.as_deref(), Some("alpha.png"), "the cache is kept for the file rewrite");
    let Some(RangeMask::Color { px, py, .. }) = r.masks[0].range else { panic!("range") };
    assert!(near(px, 0.26) && near(py, 0.73));
    let RetouchShape::Ellipse { cx, cy, size_x, size_y } = r.retouch[0].shape else { panic!("ellipse") };
    assert!(near(cx, 0.51) && near(cy, 0.48) && size_x == 0.02 && size_y == 0.02);
    let d = r.retouch[0].donor.unwrap();
    assert!(near(d[0], 0.31) && near(d[1], 0.28));
    let RetouchShape::Brush(strokes) = &r.retouch[1].shape else { panic!("retouch brush") };
    assert_eq!(strokes[0].dabs, "r 0.050000\nd 0.510000 0.480000\nf 1.000000\nd 0.610000 0.380000");
    // The colour field: a 2 x 1 grid holding 0 and 1 moved right by a
    // fiftieth of the frame = 0.02 of a cell, so the right cell reads 0.98
    // of the way from 0 to 1 and the left cell clamps to the edge.
    let f = r.colour_field.as_ref().unwrap();
    assert!(near(f.grid[0][0], 0.0) && near(f.grid[1][0], 0.98), "{:?}", f.grid);
    // And back, on everything that was not clamped or resampled.
    assert!(shift_recipe_coords(&mut r, -du, -dv));
    let back = seed();
    let (
        MaskGeometry::Radial { top, left, bottom, right, .. },
        MaskGeometry::Radial { top: t0, left: l0, bottom: b0, right: r0, .. },
    ) = (&r.masks[0].mask, &back.masks[0].mask)
    else {
        panic!("radial")
    };
    assert!(near(*top, *t0) && near(*left, *l0) && near(*bottom, *b0) && near(*right, *r0), "radial round trip");
    let (MaskGeometry::Brush { strokes: a, .. }, MaskGeometry::Brush { strokes: b, .. }) =
        (&r.masks[0].components[1].geometry, &back.masks[0].components[1].geometry)
    else {
        panic!("brush")
    };
    assert_eq!(a[0].dabs, b[0].dabs, "the dab stream round-trips on Lightroom's six-decimal grid");
    let c = r.crop.unwrap();
    assert!(near(c.left, 0.0) && near(c.top, 0.2) && near(c.right, 0.8) && near(c.bottom, 1.0), "{c:?}");
}

/// The raster half reads the picture where it came from, clamps past the
/// edge, copies exactly on an integer move and blends on a fractional one.
#[test]
fn a_raster_shift_reads_the_picture_where_it_came_from() {
    let img = image::GrayImage::from_fn(6, 4, |x, y| image::Luma([(x * 10 + y) as u8]));
    let moved = shift_luma_raster(&img, -2.0, -1.0);
    for y in 0..4u32 {
        for x in 0..6u32 {
            let (sx, sy) = ((x + 2).min(5), (y + 1).min(3));
            assert_eq!(moved.get_pixel(x, y).0[0], (sx * 10 + sy) as u8, "({x}, {y})");
        }
    }
    let half = shift_luma_raster(&img, -0.5, 0.0);
    assert_eq!(half.get_pixel(0, 0).0[0], 5, "half way between 0 and 10");
    assert_eq!(half.get_pixel(5, 0).0[0], 50, "past the right edge the edge continues");
    let same = shift_luma_raster(&img, 0.0, 0.0);
    assert_eq!(same.as_raw(), img.as_raw(), "the zero move is the identity");
}

/// One brush group with `n` strokes built from `(value, radius, flow,
/// hardness, dabs)` — the fixture every brush test below stands on.
fn probe_brush(strokes: &[(f32, f32, f32, f32, &str)]) -> MaskGeometry {
    MaskGeometry::Brush {
        name: "Brush 1".into(),
        blend_mode: 0,
        // The AGGREGATE's MaskValue: the subtract pair's other half, never
        // a strength — `mask_weight`'s `Brush` arm spells out why.
        value: 1.0,
        inverted: false,
        strokes: strokes
            .iter()
            .map(|&(value, radius, flow, center_weight, dabs)| crate::recipe::BrushStroke {
                value,
                radius,
                flow,
                center_weight,
                sync_id: "FA7459A9F5626F4881D7B730C3093F95".into(),
                dabs: dabs.into(),
            })
            .collect(),
    }
}

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

/// Real-machine probe, never run in CI (it allocates gigabytes): the
/// PORTRAIT branch's transient on a 61 MP frame — the one orientation
/// state that was unreachable for an ARW until v0.30.0, so its cost had
/// never actually been paid on a Sony file.
///
/// `orient_f32` casts to and from `Rgb32F` with zero copies
/// (`bytemuck::cast_vec`), so the whole transient is `oriented`'s own
/// `rotate270`: one fresh output frame while the source is still alive.
/// 61 MP x 3 channels x 4 bytes = 732 MB per frame, so the accounting
/// predicts a ~1.46 GB peak and no third copy. Measure with:
///
/// ```text
/// cargo test --lib -- --ignored --exact render::tests::portrait_rotation_peak_on_a_61mp_frame
/// ```
///
/// and read the printed before/after RSS, or wrap the test binary in
/// `Start-Process -PassThru -Wait` and read `PeakWorkingSet64`.
#[test]
#[ignore = "real-machine probe: allocates ~1.5 GB"]
fn portrait_rotation_peak_on_a_61mp_frame() {
    // 9504 x 6336 = the A7R IV sensor, landscape; Rotate270 turns it
    // portrait, which is exactly what an orientation-8 ARW now does.
    let (w, h) = (9504usize, 6336usize);
    let px = w * h;
    let frame_bytes = px * 3 * 4;
    let data: Vec<[f32; 3]> = vec![[0.5, 0.5, 0.5]; px];
    eprintln!(
        "one frame = {:.0} MB ({px} px); predicted peak = {:.0} MB (source + rotated copy)",
        frame_bytes as f64 / 1e6,
        2.0 * frame_bytes as f64 / 1e6
    );
    let (out, ow, oh) = orient_f32(data, w, h, Orientation::Rotate270);
    assert_eq!((ow, oh), (h, w), "the frame must come back portrait");
    assert_eq!(out.len(), px);
    assert_eq!(out.capacity() * 12, frame_bytes, "no third copy was made");
}

#[test]
fn thumbnail_binning_commutes_only_with_non_reversing_orientations() {
    // The probe behind the A7 decision to keep the working-resolution
    // cap AFTER orientation (see `render_to_image_in`) — RE-RUN with the
    // type-exact flip after the eight-state test exposed that the first
    // measurement's Transpose figure (0.48) was entirely the Rgba<u8>
    // flip adapter's quantization (U14). The true shape: `thumbnail`'s
    // integer binning uses forward bin edges, so it commutes EXACTLY
    // with pure axis swaps (Normal, Transpose) and diverges by one
    // source bin (≈1/97 on this gradient) under every orientation with
    // a REVERSAL component — mirrored bin edges of a non-integer ratio
    // don't line up. Cap-before-orientation therefore STAYS forbidden
    // (six of eight states would change preview pixels). The exact arm
    // also pins the type-exact flip: a quantizing Transpose flip shows
    // up here as ~0.5. If the reversing arm ever FAILS, the crate's
    // binning became mirror-symmetric — re-probe all eight states
    // before touching the pipeline order.
    let (w, h) = (97usize, 61usize);
    let data: Vec<[f32; 3]> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            [x as f32 / w as f32, y as f32 / h as f32, 0.0]
        })
        .collect();
    let diff = |o: Orientation| -> f32 {
        let (a, aw, ah) = {
            let (d, dw, dh) = orient_f32(data.clone(), w, h, o);
            downscale_f32(d, dw, dh, 40)
        };
        let (b, bw, bh) = {
            let (d, dw, dh) = downscale_f32(data.clone(), w, h, 40);
            orient_f32(d, dw, dh, o)
        };
        assert_eq!((aw, ah), (bw, bh), "{o:?}: dims always agree");
        a.iter()
            .zip(&b)
            .flat_map(|(p, q)| (0..3).map(move |c| (p[c] - q[c]).abs()))
            .fold(0.0f32, f32::max)
    };
    // Normal is excluded: orient_f32 is a passthrough there, so both
    // sides of the comparison would be the SAME call on equal inputs —
    // an assertion that cannot fail (R12). Transpose is the real
    // content: a pure axis swap that must commute with the binning, and
    // the arm that catches a quantizing flip (~0.5 here).
    let d = diff(Orientation::Transpose);
    assert!(d <= 1e-6, "Transpose must commute exactly, diff {d}");
    for o in [
        Orientation::HorizontalFlip,
        Orientation::VerticalFlip,
        Orientation::Rotate90,
        Orientation::Rotate180,
        Orientation::Rotate270,
        Orientation::Transverse,
    ] {
        let d = diff(o);
        assert!(
            d > 1e-4,
            "{o:?}: binning became mirror-symmetric (diff {d}) — re-probe \
             all eight states before considering cap-before-orientation"
        );
    }
}

#[test]
fn downscale_f32_is_an_unbiased_average() {
    // The working-resolution cap must not shift LEVELS. A flat field must
    // survive exactly, and a gradient's mean must be preserved: every GUI
    // preview, every retouch base and the camera-base-curve estimation all
    // run through this path, so a per-pixel offset here would wash out the
    // whole application (R12).
    let (w, h) = (97usize, 61usize);
    let flat: Vec<[f32; 3]> = vec![[0.25, 0.5, 0.75]; w * h];
    let (small, sw, sh) = downscale_f32(flat, w, h, 40);
    assert!(sw <= 40 && sh <= 40, "capped to {sw}x{sh}");
    for p in &small {
        for (c, want) in [0.25f32, 0.5, 0.75].iter().enumerate() {
            assert!(
                (p[c] - want).abs() < 1e-4,
                "flat field must survive the cap unchanged: {p:?}"
            );
        }
    }
    let ramp: Vec<[f32; 3]> = (0..w * h)
        .map(|i| {
            let v = (i % w) as f32 / (w - 1) as f32;
            [v; 3]
        })
        .collect();
    let mean_in = ramp.iter().map(|p| p[0] as f64).sum::<f64>() / (w * h) as f64;
    let (small, sw, sh) = downscale_f32(ramp, w, h, 40);
    let mean_out = small.iter().map(|p| p[0] as f64).sum::<f64>() / (sw * sh) as f64;
    assert!(
        (mean_out - mean_in).abs() < 0.02,
        "the cap must preserve the mean level: {mean_in} -> {mean_out}"
    );
    // AVERAGING, not merely level. Both arms above pass for a
    // nearest-neighbour sampler (measured: top-left 0.0023 and
    // bottom-right 0.0125 off the mean, inside the 0.02 budget, and a
    // flat field survives point sampling exactly) — so "optimising" the
    // inner loop into `*px = data[bottom * w + left]` would keep the
    // suite green while every preview started aliasing.
    //
    // One lit source pixel in a dark field: a box filter spreads it over
    // its whole window, so the output lands at 1/window_area — never at 0
    // (a point sampler that missed it) and never at 1 (one that hit it).
    let mut spike = vec![[0.0f32; 3]; w * h];
    spike[(h / 2) * w + w / 2] = [1.0; 3];
    let (small, _sw, _sh) = downscale_f32(spike, w, h, 40);
    let peak = small.iter().map(|p| p[0]).fold(0.0f32, f32::max);
    let lit = small.iter().filter(|p| p[0] > 0.0).count();
    assert_eq!(lit, 1, "exactly one output window contains the lit pixel");
    // Bin windows are ceil-based, so this fixture (97x61 -> 40x25, ratios
    // 2.425 and 2.44) gives each output pixel 2 or 3 source columns and
    // rows: the lit sample is therefore averaged over 4..=9 of them.
    assert!(
        (1.0 / 9.0..=1.0 / 4.0).contains(&peak),
        "one lit pixel must be AVERAGED over its 4..=9 sample window, got {peak}"
    );
}

#[test]
fn mask_coverage_reports_the_engine_weight() {
    use crate::recipe::{LocalAdjustment, MaskGeometry, RangeMask};
    // (a) A top→bottom linear gradient over a flat grey reference: zero at
    // the top row, ~full at the bottom, ~half in the middle.
    let grey = DynamicImage::ImageRgb8(RgbImage::from_pixel(20, 20, image::Rgb([120, 120, 120])));
    let grad = LocalAdjustment {
        mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.0, full_x: 0.5, full_y: 1.0 },
        ..Default::default()
    };
    let cov = mask_coverage(&grad, &grey, MaskFrame::AsRendered);
    // Rows sample at their CENTRES, ny = (y + 0.5)/20, so this 0→1
    // gradient reads t = 0.025 / 0.525 / 0.975 at rows 0 / 10 / 19; the
    // shipped `Measured` profile maps those to 0.00075 / 0.47703 / 0.99768
    // and quantises to `round(w · 255)` = 0 / 122 / 254 (Clamped gave
    // 6 / 134 / 249; the unwarped smoothstep gave 0 / 137 / 255). Pinned
    // EXACTLY, both because the arithmetic is exact and because rows 10
    // and 19 still separate the conventions: `y/h` would give
    // t = 0.0 / 0.5 / 0.95 → 0 / 112 / 249. Under Clamped the old
    // `assert_eq!(…, 0)` on row 0 was the whole reason this test caught
    // R29 C2 rather than sleeping through it; under any eased profile row
    // 0 rounds to 0 either way, so rows 10 and 19 carry that duty now. Row
    // 19 reading 254 rather than 255 IS the warp: the last sample sits at
    // t = 0.975, and smoothstep(0.975^1.124) = 0.99768 is under 254.5/255.
    assert_eq!(cov.get_pixel(10, 0)[0], 0, "the zero end is flat at the handle");
    assert_eq!(cov.get_pixel(10, 19)[0], 254, "the full end reaches its plateau");
    assert_eq!(cov.get_pixel(10, 10)[0], 122, "the midpoint sits off the linear centre");

    // (b) amount halves the whole map; inversion flips its direction.
    let half = LocalAdjustment { amount: 0.5, ..grad.clone() };
    assert!((mask_coverage(&half, &grey, MaskFrame::AsRendered).get_pixel(10, 19)[0] as i32 - 128).abs() < 15);
    let inv = LocalAdjustment { inverted: true, ..grad.clone() };
    let icov = mask_coverage(&inv, &grey, MaskFrame::AsRendered);
    assert!(icov.get_pixel(10, 0)[0] > 235 && icov.get_pixel(10, 19)[0] < 20);

    // (c) A luminance range gates the map by the REFERENCE pixels: with a
    // bright-only range, the dark half of the reference reads 0 even where
    // the geometry is at full strength.
    let split = DynamicImage::ImageRgb8(RgbImage::from_fn(20, 20, |x, _| {
        if x < 10 { image::Rgb([30, 30, 30]) } else { image::Rgb([220, 220, 220]) }
    }));
    let ranged = LocalAdjustment {
        // Degenerate linear (zero == full) = weight 1 everywhere.
        mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.5, full_x: 0.5, full_y: 0.5 },
        range: Some(RangeMask::Luminance { lo_outer: 0.5, lo: 0.6, hi: 1.0, hi_outer: 1.0 }),
        ..Default::default()
    };
    let rcov = mask_coverage(&ranged, &split, MaskFrame::AsRendered);
    assert_eq!(rcov.get_pixel(3, 10)[0], 0, "dark side gated out");
    assert!(rcov.get_pixel(16, 10)[0] > 235, "bright side kept: {}", rcov.get_pixel(16, 10)[0]);
}

#[test]
fn angled_linear_mask_matches_the_pixel_metric_closed_form() {
    let g = MaskGeometry::Linear {
        zero_x: 0.15,
        zero_y: 0.20,
        full_x: 0.85,
        full_y: 0.80,
    };
    let (w, h) = (300.0f32, 200.0f32);
    let (zx, zy, fx, fy) = match &g {
        MaskGeometry::Linear { zero_x, zero_y, full_x, full_y } => (*zero_x, *zero_y, *full_x, *full_y),
        _ => unreachable!(),
    };
    let (vx, vy) = (fx - zx, fy - zy);
    let (px, py) = (vx * w, vy * h);
    let den = px * px + py * py;
    for (nx, ny) in [(0.30, 0.25), (0.55, 0.50), (0.75, 0.65)] {
        let dx = (nx - zx) * w;
        let dy = (ny - zy) * h;
        let want = linear_coverage((dx * px + dy * py) / den, LINEAR_FALLOFF);
        let got = mask_weight_in(&g, nx, ny, None, None, (w, h));
        assert!((got - want).abs() < 1e-6, "({nx},{ny}): got {got}, want {want}");
        let normalized = mask_weight(&g, nx, ny, None);
        assert!((got - normalized).abs() > 1e-3, "({nx},{ny}) did not expose aspect skew");
    }
}

#[test]
fn axis_aligned_linear_coverage_is_byte_stable() {
    let (w, h) = (13u32, 9u32);
    let g = MaskGeometry::Linear {
        zero_x: 0.4,
        zero_y: 0.1,
        full_x: 0.4,
        full_y: 0.9,
    };
    let m = LocalAdjustment { mask: g.clone(), ..Default::default() };
    let reference = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb([120, 120, 120])));
    let got = mask_coverage(&m, &reference, MaskFrame::AsRendered);
    let mut want = image::GrayImage::new(w, h);
    for (x, y, px) in want.enumerate_pixels_mut() {
        let weight = mask_weight(
            &g,
            (x as f32 + MASK_SAMPLE_CENTRE) / w as f32,
            (y as f32 + MASK_SAMPLE_CENTRE) / h as f32,
            None,
        );
        *px = image::Luma([(weight * 255.0).round() as u8]);
    }
    assert_eq!(got.as_raw(), want.as_raw(), "axis-aligned coverage changed byte-for-byte");
}

#[test]
fn linear_coverage_clamped_is_byte_identical_to_head() {
    // Keep the pre-refactor ramp expression in the test itself. This is a
    // code snapshot, rather than a file fixture that could be regenerated
    // with the new implementation by mistake.
    for i in 0..=4096u32 {
        let t = (i as f32 - 512.0) / 3072.0;
        let head = t.clamp(0.0, 1.0);
        let got = linear_coverage(t, LinearFalloff::Clamped);
        // Exact f32 identity on purpose: the 16-bit form of this check
        // went green under hand mutation M-L2 (2026-08-28, the [0,1] clamp
        // removed) because a saturating integer cast turns negative and
        // above-one coverage into the same 0 / 65535 the head ramp gives.
        assert_eq!(got.to_bits(), head.to_bits(), "clamped coverage changed at t={t}");
    }
}

#[test]
fn linear_coverage_eased_is_c1_at_both_ends() {
    let n = 2000usize;
    let values: Vec<u16> = (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1) as f32;
            (linear_coverage(t, LinearFalloff::Eased) * 65535.0).round() as u16
        })
        .collect();
    let slopes: Vec<i32> = values.windows(2).map(|p| p[1] as i32 - p[0] as i32).collect();
    let through: f32 = slopes[400..1600].iter().map(|&v| v as f32).sum::<f32>() / 1200.0;
    assert!(through > 1.0, "the 16-bit ramp must have a measurable slope");
    let turnover = |part: &[i32]| {
        part.iter()
            .take(80)
            .take_while(|&&s| (s as f32 - through).abs() >= 0.25 * through)
            .count()
    };
    let max_edge_jump = |part: &[i32]| {
        part.windows(2).take(80).map(|p| (p[1] - p[0]).abs()).max().unwrap_or(0)
    };
    assert!(max_edge_jump(&slopes) <= 2, "eased full-end first difference is discontinuous");
    assert!(max_edge_jump(&slopes[slopes.len() - 81..]) <= 2, "eased zero-end first difference is discontinuous");
    assert!(turnover(&slopes) > 2, "eased full end turns over in one row");
    assert!(turnover(&slopes[slopes.len() - 80..]) > 2, "eased zero end turns over in one row");
    assert_eq!(linear_coverage(0.0, LinearFalloff::Eased), 0.0);
    assert_eq!(linear_coverage(1.0, LinearFalloff::Eased), 1.0);
}

#[test]
fn linear_coverage_profiles_agree_at_the_handles() {
    for &t in &[0.0, 1.0] {
        assert_eq!(linear_coverage(t, LinearFalloff::Clamped), linear_coverage(t, LinearFalloff::Eased));
    }
}

#[test]
fn shipped_linear_falloff_is_eased() {
    // The shipped arm is `Measured` — Hermite smoothstep on the measured
    // abscissa t^LINEAR_FALLOFF_WARP (me6-2026-09 group B, 12 gradients).
    assert_eq!(LINEAR_FALLOFF, LinearFalloff::Measured);
    // …and it is still an EASING, which is what this test's name buys:
    // C1 at both handles, strictly monotone in between, and equal to the
    // other two profiles exactly at t = 0 and t = 1. A mutation pointing
    // LINEAR_FALLOFF at Clamped fails the slope pair as well as the
    // assert_eq above.
    for &t in &[0.0f32, 1.0] {
        assert_eq!(linear_coverage(t, LINEAR_FALLOFF), linear_coverage(t, LinearFalloff::Clamped));
    }
    let d = 1e-3;
    let slope = |t: f32| (linear_coverage(t + d, LINEAR_FALLOFF) - linear_coverage(t, LINEAR_FALLOFF)) / d;
    assert!(slope(0.0) < 0.02, "zero handle is not C1: {}", slope(0.0));
    assert!(slope(1.0 - d) < 0.02, "full handle is not C1: {}", slope(1.0 - d));
    let mut prev = -1.0f32;
    for i in 0..=1000 {
        let got = linear_coverage(i as f32 / 1000.0, LINEAR_FALLOFF);
        assert!(got > prev, "not monotone at t = {}", i as f32 / 1000.0);
        prev = got;
    }
}

#[test]
fn linear_ramp_has_a_single_definition() {
    let src = include_str!("mask_weight.rs");
    let metric = &src[src.find("fn mask_weight_metric").unwrap()..src.find("/// Mask coverage").unwrap()];
    let weight = &src[src.find("fn mask_weight(g:").unwrap()..src.find("fn combined_mask_weight").unwrap()];
    let metric_linear = &metric[metric.find("MaskGeometry::Linear").unwrap()..metric.find("_ => mask_weight").unwrap()];
    let weight_linear = &weight[weight.find("MaskGeometry::Linear").unwrap()..weight.find("// `roundness`").unwrap()];
    assert_eq!(metric_linear.matches("linear_coverage(").count(), 2, "metric has a second ramp definition");
    assert_eq!(weight_linear.matches("linear_coverage(").count(), 1, "weight has a second ramp definition");
    assert!(!metric_linear.contains(".clamp(0.0, 1.0)"), "metric keeps an inline linear clamp");
    assert!(!weight_linear.contains(".clamp(0.0, 1.0)"), "weight keeps an inline linear clamp");
}

#[test]
fn radial_mask_renders_byte_identical_to_the_clamped_baseline() {
    let (w, h) = (48u32, 32u32);
    let src = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x * 3 + y * 2) as u8, (x * 5) as u8, (y * 7) as u8])
    }));
    let mask = crate::recipe::LocalAdjustment {
        mask: MaskGeometry::Radial { top: 0.1, left: 0.1, bottom: 0.9, right: 0.9, feather: 0.4, roundness: 0.0, flipped: false, angle: 0.0, midpoint: 50.0, mask_version: 2 },
        ..Default::default()
    };
    let got = mask_coverage(&mask, &src, MaskFrame::AsRendered);
    let want = image::GrayImage::from_fn(w, h, |x, y| {
        let nx = (x as f32 + MASK_SAMPLE_CENTRE) / w as f32;
        let ny = (y as f32 + MASK_SAMPLE_CENTRE) / h as f32;
        let mut weight = mask_weight(&mask.mask, nx, ny, None);
        if mask.inverted {
            weight = 1.0 - weight;
        }
        image::Luma([(weight * 255.0).round() as u8])
    });
    assert_eq!(got.as_raw(), want.as_raw(), "mask coverage changed from the clamped baseline");
}

#[test]
fn bitmap_mask_renders_byte_identical_to_the_clamped_baseline() {
    let (w, h) = (48u32, 32u32);
    let src = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x * 3 + y * 2) as u8, (x * 5) as u8, (y * 7) as u8])
    }));
    // Own directory, not the bare temp root (see `fixture_mask_path`).
    let raster_dir =
        std::env::temp_dir().join(format!("autoshade-linear-baseline-{}", std::process::id()));
    std::fs::create_dir_all(&raster_dir).unwrap();
    let raster_path = raster_dir.join("mask.png");
    let raster = image::GrayImage::from_fn(7, 5, |x, y| image::Luma([((x + y) * 20) as u8]));
    raster.save(&raster_path).unwrap();
    let mask = crate::recipe::LocalAdjustment {
        mask: MaskGeometry::Bitmap { path: raster_path.to_string_lossy().into_owned() },
        ..Default::default()
    };
    let got = mask_coverage(&mask, &src, MaskFrame::AsRendered);
    let bmp = load_mask_bitmap(&mask.mask, &crate::diag::dropped());
    let want = image::GrayImage::from_fn(w, h, |x, y| {
        let nx = (x as f32 + MASK_SAMPLE_CENTRE) / w as f32;
        let ny = (y as f32 + MASK_SAMPLE_CENTRE) / h as f32;
        let mut weight = mask_weight(&mask.mask, nx, ny, bmp.as_deref());
        if mask.inverted {
            weight = 1.0 - weight;
        }
        image::Luma([(weight * 255.0).round() as u8])
    });
    assert_eq!(got.as_raw(), want.as_raw(), "mask coverage changed from the clamped baseline");
    let _ = std::fs::remove_file(raster_path);
}

#[test]
fn linear_mask_renders_the_eased_ramp() {
    let (w, h) = (48u32, 32u32);
    let src = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x * 3 + y * 2) as u8, (x * 5) as u8, (y * 7) as u8])
    }));
    let mask = crate::recipe::LocalAdjustment {
        mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 1.0, full_x: 0.5, full_y: 0.0 },
        ..Default::default()
    };
    let got = mask_coverage(&mask, &src, MaskFrame::AsRendered);
    let want = image::GrayImage::from_fn(w, h, |x, y| {
        let nx = (x as f32 + MASK_SAMPLE_CENTRE) / w as f32;
        let ny = (y as f32 + MASK_SAMPLE_CENTRE) / h as f32;
        let (zero_x, zero_y, full_x, full_y) = (0.5, 1.0, 0.5, 0.0);
        let t = (((nx - zero_x) * (full_x - zero_x) + (ny - zero_y) * (full_y - zero_y))
            / ((full_x - zero_x).powi(2) + (full_y - zero_y).powi(2)))
            .clamp(0.0, 1.0);
        image::Luma([(linear_coverage(t, LINEAR_FALLOFF) * 255.0).round() as u8])
    });
    assert_eq!(got.as_raw(), want.as_raw(), "linear mask coverage did not use the shipped ramp");

    let byte = |value: f32| (value * 255.0).round() as u8;
    assert_eq!(linear_coverage(0.25, LinearFalloff::Eased), 0.15625);
    assert_eq!(linear_coverage(0.25, LinearFalloff::Clamped), 0.25);
    assert_ne!(byte(linear_coverage(0.25, LinearFalloff::Eased)), byte(linear_coverage(0.25, LinearFalloff::Clamped)));
    for t in [-1.0, 0.0, 1.0, 2.0] {
        assert_eq!(linear_coverage(t, LinearFalloff::Eased), linear_coverage(t, LinearFalloff::Clamped));
    }
}

#[test]
fn shipped_linear_ramp_is_eased_end_to_end() {
    // The probe geometry (vertical gradient, zero at 0.80, full at 0.35,
    // −2 EV) through the public f32 develop path. The tone pass blends
    // `p·(1 − w) + t·w`, so on a flat grey the mask weight is recovered
    // EXACTLY as `(base − row) / (base − full_plateau)` — no assumption
    // about the exposure model, and no 8-bit quantisation. The expected
    // profile is the LITERAL Hermite smoothstep on the LITERAL measured
    // abscissa, not `linear_coverage` and not LINEAR_FALLOFF_WARP, so a
    // mutation of the shipped arm cannot rewrite its own oracle.
    let (w, h) = (4usize, 1000usize);
    let base = 0.5_f32;
    let recipe = EditRecipe {
        masks: vec![crate::recipe::LocalAdjustment {
            mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.80, full_x: 0.5, full_y: 0.35 },
            exposure_ev: -2.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut px = vec![[base; 3]; w * h];
    apply_develop_anon(&mut px, w, h, &recipe);
    let rows: Vec<f32> = (0..h).map(|y| px[y * w + w / 2][1]).collect();
    let full_plateau = rows[100];
    assert!(full_plateau < base - 0.2, "full end must darken by −2 EV: {full_plateau}");
    assert_eq!(rows[950].to_bits(), base.to_bits(), "zero end must be untouched");
    let coverage: Vec<f32> = rows.iter().map(|r| (base - r) / (base - full_plateau)).collect();
    let t_of = |y: usize| {
        let ny = (y as f32 + MASK_SAMPLE_CENTRE) / h as f32;
        ((0.80 - ny) / (0.80 - 0.35)).clamp(0.0, 1.0)
    };
    // (a) every row IS the literal warped smoothstep of its projected t
    // (the 0.001 work floor and the tone LUT leave ≤ 2e-3 of slack).
    for (y, &got) in coverage.iter().enumerate() {
        let t = t_of(y);
        let u = t.powf(1.124);
        let want = u * u * (3.0 - 2.0 * u);
        assert!((got - want).abs() < 2e-3, "row {y}: t {t:.4} rendered {got:.5}, warped smoothstep {want:.5}");
    }
    // (b) C1 at BOTH handles: the coverage slope over the ten rows just
    // inside each handle is ≤ 0.1 of the mid-ramp slope (Clamped: ≈ 1.0;
    // `t²`: ≈ 0.0 at the zero end but ≈ 2.0 at the full end).
    let full_row = (0.35 * h as f32) as usize;
    let zero_row = (0.80 * h as f32) as usize;
    let mid = (full_row + zero_row) / 2;
    let slope = |a: usize, b: usize| ((coverage[b] - coverage[a]) / (b - a) as f32).abs();
    let mid_slope = slope(mid - 5, mid + 5);
    let full_end = slope(full_row + 1, full_row + 11);
    let zero_end = slope(zero_row - 11, zero_row - 1);
    assert!(full_end < 0.1 * mid_slope, "full-end slope {full_end:.2e} is not eased vs mid {mid_slope:.2e}");
    assert!(zero_end < 0.1 * mid_slope, "zero-end slope {zero_end:.2e} is not eased vs mid {mid_slope:.2e}");
    // (c) the shipped midpoint is 1.535× the linear slope of the same
    // span: 1.5 from the smoothstep, × 1.0314 from d(t^1.124)/dt at t = ½.
    let linear_slope = 1.0 / (zero_row - full_row) as f32;
    let ratio = mid_slope / linear_slope;
    assert!((1.52..=1.55).contains(&ratio), "mid-ramp slope ratio {ratio:.4} is not ~1.535");
}

#[test]
fn probe_fixture_round_trips_through_xmp() {
    let recipe = EditRecipe {
        masks: vec![crate::recipe::LocalAdjustment {
            mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.80, full_x: 0.5, full_y: 0.35 },
            exposure_ev: -2.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let xmp = crate::xmp::recipe_to_xmp(&recipe);
    let back = crate::xmp::xmp_to_recipe(&xmp);
    assert_eq!(back.masks.len(), 1);
    let MaskGeometry::Linear { zero_x, zero_y, full_x, full_y } = back.masks[0].mask else { panic!("probe mask was not linear") };
    assert!((zero_x - 0.5).abs() < 1e-6 && (zero_y - 0.80).abs() < 1e-6);
    assert!((full_x - 0.5).abs() < 1e-6 && (full_y - 0.35).abs() < 1e-6);
    assert_eq!(back.masks[0].exposure_ev, -2.0);
    if crate::config::live_env_os("AUTOSHADE_GENERATE_LINEAR_PROBE").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/linear-falloff/probe");
        std::fs::create_dir_all(&dir).unwrap();
        let encoded = (linear_to_srgb(0.18) * 65535.0).round() as u16;
        let image = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_pixel(3000, 2000, image::Rgb([encoded; 3]));
        image::DynamicImage::ImageRgb16(image).save(dir.join("probe.tif")).unwrap();
        std::fs::write(dir.join("probe.xmp"), xmp.as_bytes()).unwrap();
        std::fs::write(dir.join("probe-recipe.json"), serde_json::to_vec_pretty(&back).unwrap()).unwrap();
    }
}

#[test]
fn gui_coverage_overlay_matches_an_angled_linear_render_weight() {
    let (w, h) = (96u32, 64u32);
    let base = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb([255, 255, 255])));
    let adj = LocalAdjustment {
        mask: MaskGeometry::Linear {
            zero_x: 0.08,
            zero_y: 0.15,
            full_x: 0.92,
            full_y: 0.85,
        },
        exposure_ev: -4.0,
        ..Default::default()
    };
    let recipe = EditRecipe { masks: vec![adj.clone()], ..Default::default() };
    let coverage = mask_coverage(&adj, &base, MaskFrame::AsRendered);
    let rendered =
        develop_preview_framed(&base, &recipe, &crate::diag::pixels(), MaskFrame::AsRendered, None, false)
            .to_rgb8();
    let (mut claimed, mut agreed, mut clear, mut clean) = (0u32, 0u32, 0u32, 0u32);
    for (x, y, p) in coverage.enumerate_pixels() {
        let lit = rendered.get_pixel(x, y).0[1];
        if p[0] > 200 {
            claimed += 1;
            if lit < 160 {
                agreed += 1;
            }
        } else if p[0] < 20 {
            clear += 1;
            if lit > 200 {
                clean += 1;
            }
        }
    }
    assert!(claimed > 500 && clear > 200, "premise: {claimed} covered / {clear} clear px");
    assert!(agreed * 100 >= claimed * 97, "angled overlay coverage disagrees: {agreed}/{claimed}");
    assert!(clean * 100 >= clear * 97, "angled overlay shows a clean pixel as covered: {clean}/{clear}");
}

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

/// Manual, machine-relative regression probe for the GUI's engine hot path.
/// Ignored in normal CI because wall-clock budgets are hardware-dependent;
/// run release-only and compare same-machine ratios:
/// `cargo test --release --lib preview_mask_perf_probe -- --ignored --nocapture`.
/// The checksum prevents a future fast path from "winning" by skipping work.
#[test]
#[ignore]
fn preview_mask_perf_probe() {
    use std::time::Instant;

    let (w, h) = (1280u32, 853u32);
    let base = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        let fx = x as f32 / (w - 1) as f32;
        let fy = y as f32 / (h - 1) as f32;
        Rgb([
            (255.0 * (0.15 + 0.75 * fx)).round() as u8,
            (255.0 * (0.12 + 0.65 * fy)).round() as u8,
            (255.0 * (0.18 + 0.55 * (1.0 - fx * fy))).round() as u8,
        ])
    }));
    // Process-unique (the ./out fixture race — see fit_zoned's
    // fixture_mask_path): a concurrent `cargo test` deleted this mask
    // mid-run, turning the zone inert and the measurement meaningless.
    let mask_path =
        {
            // Own directory, not the bare temp root (see `fixture_mask_path`).
            let d = std::env::temp_dir()
                .join(format!("autoshade-preview-perf-mask-{}", std::process::id()));
            std::fs::create_dir_all(&d).unwrap();
            d.join("mask.png")
        };
    image::GrayImage::from_fn(w / 4, h / 4, |x, _| {
        image::Luma([((x as f32 / (w / 4 - 1) as f32) * 255.0).round() as u8])
    })
    .save(&mask_path)
    .unwrap();
    let zone = |inverted| LocalAdjustment {
        mask: MaskGeometry::Bitmap { path: mask_path.to_string_lossy().into_owned() },
        inverted,
        exposure_ev: if inverted { -0.35 } else { 0.45 },
        contrast: if inverted { 8.0 } else { -6.0 },
        saturation: if inverted { -4.0 } else { 9.0 },
        color_gains: Some(if inverted { [1.35, 0.88, 0.62] } else { [1.15, 0.96, 0.78] }),
        ..Default::default()
    };
    let no_colour_zone = |inverted| LocalAdjustment {
        mask: MaskGeometry::Bitmap { path: mask_path.to_string_lossy().into_owned() },
        inverted,
        exposure_ev: if inverted { -0.35 } else { 0.45 },
        contrast: if inverted { 8.0 } else { -6.0 },
        saturation: if inverted { -4.0 } else { 9.0 },
        ..Default::default()
    };
    let recipes = [
        ("zero", EditRecipe { exposure_ev: 0.2, saturation: 8.0, ..Default::default() }),
        ("one_no_colour", EditRecipe {
            exposure_ev: 0.2,
            saturation: 8.0,
            masks: vec![no_colour_zone(false)],
            ..Default::default()
        }),
        ("one", EditRecipe {
            exposure_ev: 0.2,
            saturation: 8.0,
            masks: vec![zone(false)],
            ..Default::default()
        }),
        ("shared_pair_no_colour", EditRecipe {
            exposure_ev: 0.2,
            saturation: 8.0,
            masks: vec![no_colour_zone(false), no_colour_zone(true)],
            ..Default::default()
        }),
        ("shared_pair", EditRecipe {
            exposure_ev: 0.2,
            saturation: 8.0,
            masks: vec![zone(false), zone(true)],
            ..Default::default()
        }),
    ];
    for (name, recipe) in recipes {
        let _ = develop_preview(&base, &recipe); // warm bitmap decode cache
        let start = Instant::now();
        let mut checksum = 0u64;
        const N: usize = 5;
        for _ in 0..N {
            let out = develop_preview(&base, &recipe).to_rgb8();
            checksum = out
                .as_raw()
                .iter()
                .step_by(997)
                .fold(checksum, |acc, &v| acc.wrapping_mul(16777619) ^ v as u64);
        }
        eprintln!(
            "PERF preview/{name}: {:.2} ms/frame checksum={checksum}",
            start.elapsed().as_secs_f64() * 1000.0 / N as f64,
        );
    }
    std::fs::remove_file(&mask_path).ok();
}

#[test]
fn render_to_file_clamps_extreme_finite_mask_geometry_before_pixel_work() {
    use crate::recipe::MaskGeometry;

    let dir = std::env::temp_dir().join(format!(
        "autoshade-render-clamp-{}-{}",
        std::process::id(),
        crate::store::next_tmp_seq()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("src.png");
    DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 4, image::Rgb([120, 120, 120])))
        .save(&src)
        .unwrap();

    let wild = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Linear {
                zero_x: 1e30,
                zero_y: 1e30,
                full_x: -1e30,
                full_y: -1e30,
            },
            exposure_ev: 1.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut clamped = wild.clone();
    clamped.clamp();
    let wild_out = dir.join("wild.png");
    let clamped_out = dir.join("clamped.png");
    render_to_file(&src, &wild, &wild_out, None, None, crate::diag::stderr()).unwrap();
    render_to_file(&src, &clamped, &clamped_out, None, None, crate::diag::stderr()).unwrap();

    let got = image::open(&wild_out).unwrap().to_rgb16();
    let expected = image::open(&clamped_out).unwrap().to_rgb16();
    assert_eq!(got.as_raw(), expected.as_raw());
    assert!(got.as_raw().iter().all(|&channel| channel != 0));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A PNG that carries ONLY its header (signature + IHDR + an empty IDAT),
/// so a fixture can claim 61 MP dimensions in 45 bytes.
///
/// Legitimate for the budget projection under test because that projection
/// is header-only BY DESIGN (`raster_projected_bytes` never decodes), and
/// necessary because the honest alternative is not free: encoding a real
/// 9504×6336 grayscale PNG measured 3.16 s and a 60 MB allocation per
/// fixture (probed on this machine), twice over in the scenario below.
fn write_header_only_png(path: &std::path::Path, w: u32, h: u32) {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in bytes {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc_over = kind.to_vec();
        crc_over.extend_from_slice(data);
        out.extend_from_slice(&crc32(&crc_over).to_be_bytes());
        out
    }
    // IHDR: width, height, 8-bit, colour type 0 (grayscale), deflate,
    // adaptive filtering, no interlace — the same shape a mask raster has.
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
    let mut file = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    file.extend_from_slice(&chunk(b"IHDR", &ihdr));
    // The dimension reader stops at the first image-data chunk HEADER, so
    // an empty IDAT is enough to make the header complete and parsable.
    file.extend_from_slice(&chunk(b"IDAT", &[]));
    std::fs::write(path, &file).expect("fixture written");
}

/// R22 H1: the mask-refine precheck must ask the question the LOADER asks —
/// the aggregate one. The refined raster is charged next to the recipe's
/// other active rasters, so the SECOND full-resolution refine on a 61 MP
/// photo is refused instead of publishing a raster that `render_to_file`
/// then bails on and `develop_preview` silently drops.
///
/// The arithmetic, spelled out (61 MP Sony A7R: 9504×6336 = 60,217,344 px):
/// one raster projects 60,217,344 × 4 = 240,869,376 B, which fits the
/// 268,435,456 B (256 MiB) budget; two project 481,738,752 B, which does
/// not. The old single-raster judgement said yes to both.
#[test]
fn a_second_full_resolution_refine_is_refused_by_the_aggregate_budget() {
    use crate::recipe::MaskGeometry;

    let dir = std::env::temp_dir().join(format!(
        "autoshade-refine-budget-{}-{}",
        std::process::id(),
        crate::store::next_tmp_seq()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    // Two masks, both active, each with its own segmentation raster; `sky`
    // is the one that gets refined to full resolution first.
    let sky_full = dir.join("mask-sky-refined.png");
    let sky_small = dir.join("mask-sky.png");
    let ground_small = dir.join("mask-ground.png");
    write_header_only_png(&sky_full, 9504, 6336);
    for p in [&sky_small, &ground_small] {
        image::GrayImage::from_pixel(64, 48, image::Luma([255])).save(p).unwrap();
    }
    let mask = |name: &str, path: &std::path::Path| LocalAdjustment {
        name: name.into(),
        mask: MaskGeometry::Bitmap { path: path.display().to_string() },
        exposure_ev: 1.0,
        ..Default::default()
    };

    // Leg 1 — refining `sky`: its own small raster is the one being
    // REPLACED, so the sum is `ground`'s 12,288 B plus the incoming
    // 240,869,376 B. Fits.
    let pre_refine = EditRecipe {
        masks: vec![mask("sky", &sky_small), mask("ground", &ground_small)],
        ..Default::default()
    };
    assert!(
        mask_raster_write_fits_budget(
            &pre_refine,
            Some(&sky_small.display().to_string()),
            9504,
            6336
        ),
        "the FIRST full-resolution refine fits: 240,881,664 B ≤ {MASK_RASTER_BUDGET_BYTES} B"
    );

    // Leg 2 — now refining `ground` while `sky` holds its 61 MP raster:
    // 240,869,376 B already committed + 240,869,376 B incoming. Refused.
    let recipe = EditRecipe {
        masks: vec![mask("sky", &sky_full), mask("ground", &ground_small)],
        ..Default::default()
    };
    assert!(
        !mask_raster_write_fits_budget(
            &recipe,
            Some(&ground_small.display().to_string()),
            9504,
            6336
        ),
        "the SECOND full-resolution refine must be refused: 481,738,752 B > \
         {MASK_RASTER_BUDGET_BYTES} B"
    );
    // And the refusal is the AGGREGATE's, not this file's: the same raster
    // alone is still fine (which is exactly why the single-raster judgement
    // said yes).
    assert!(
        mask_raster_write_fits_budget(&EditRecipe::default(), None, 9504, 6336),
        "one 61 MP raster on its own fits — the aggregate is what refuses"
    );
    // A mask the engine will not render is not charged (the loader's own
    // filter): muting `sky` frees its raster's share.
    let mut muted = recipe.clone();
    muted.masks[0].enabled = false;
    assert!(
        mask_raster_write_fits_budget(
            &muted,
            Some(&ground_small.display().to_string()),
            9504,
            6336
        ),
        "an inactive mask's raster is never loaded, so it must not be charged"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_active_raster_set_over_its_budget_is_refused_before_pixel_work() {
    use crate::recipe::MaskGeometry;

    let dir = std::env::temp_dir().join(format!(
        "autoshade-raster-budget-{}-{}",
        std::process::id(),
        crate::store::next_tmp_seq()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.png");
    let second = dir.join("second.png");
    image::GrayImage::from_pixel(1, 1, image::Luma([255])).save(&first).unwrap();
    image::GrayImage::from_pixel(1, 1, image::Luma([255])).save(&second).unwrap();
    let recipe = EditRecipe {
        masks: vec![
            LocalAdjustment {
                name: "first".into(),
                mask: MaskGeometry::Bitmap { path: first.display().to_string() },
                exposure_ev: 1.0,
                ..Default::default()
            },
            LocalAdjustment {
                name: "second".into(),
                mask: MaskGeometry::Bitmap { path: second.display().to_string() },
                exposure_ev: 1.0,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let Err(error) = load_mask_raster_snapshot_with_budget(&recipe, 1, true, &crate::diag::pixels()) else {
        panic!("two decoded bytes must exceed a one-byte snapshot budget");
    };
    assert!(
        error.to_string().contains("1-byte aggregate budget"),
        "{error:#}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deleting_a_raster_after_snapshot_construction_does_not_change_the_render() {
    use crate::recipe::MaskGeometry;

    let dir = std::env::temp_dir().join(format!(
        "autoshade-raster-snapshot-{}-{}",
        std::process::id(),
        crate::store::next_tmp_seq()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let mask = dir.join("mask.png");
    image::GrayImage::from_pixel(2, 2, image::Luma([255])).save(&mask).unwrap();
    let recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Bitmap { path: mask.display().to_string() },
            exposure_ev: 1.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let snapshot = load_mask_raster_snapshot(&recipe, &crate::diag::pixels()).unwrap();
    let untouched = vec![[0.25, 0.25, 0.25]; 4];
    let mut before_delete = untouched.clone();
    apply_develop_with_rasters(&mut before_delete, 2, 2, &recipe, &snapshot, MaskFrame::AsRendered, FilmScale::NATIVE, false);
    std::fs::remove_file(&mask).unwrap();
    let mut after_delete = untouched.clone();
    apply_develop_with_rasters(&mut after_delete, 2, 2, &recipe, &snapshot, MaskFrame::AsRendered, FilmScale::NATIVE, false);
    assert_eq!(after_delete, before_delete);
    assert_ne!(after_delete, untouched, "the retained white mask must still apply");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_staged_encode_leaves_the_existing_target_intact() {
    let dir = std::env::temp_dir().join(format!(
        "autoshade-staged-failure-{}-{}",
        std::process::id(),
        crate::store::next_tmp_seq()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("preview.jpg");
    std::fs::write(&target, b"previous deliverable").unwrap();

    let result = stage_and_publish(&target, |staged| {
        std::fs::write(staged, b"partial new bytes")?;
        Err(anyhow::anyhow!("encoder failed"))
    });
    assert!(result.is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"previous deliverable");
    assert!(
        std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .all(|entry| !entry.file_name().to_string_lossy().contains(".tmp."))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tiled_guided_refine_matches_the_whole_frame_result_across_every_seam() {
    let (w, h) = (53u32, 47u32);
    let tile_edge = 19usize;
    assert!(
        w as usize > 2 * tile_edge && h as usize > 2 * tile_edge,
        "the fixture must cross two tile seams in each axis"
    );

    let guide = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        let base: u8 = if x + y / 3 < w / 2 { 28 } else { 210 };
        image::Rgb([
            base,
            base.saturating_add(((x * 5 + y * 3) % 17) as u8),
            base.saturating_sub(((x * 2 + y * 7) % 13) as u8),
        ])
    }));
    let small = image::GrayImage::from_fn(9, 7, |x, y| {
        image::Luma([((x * 37 + y * 53 + (x * y % 7) * 19) % 256) as u8])
    });

    let tiled = refine_mask_guided_tiled(&small, &guide, 2, 1e-2, tile_edge);
    let reference =
        refine_mask_guided_tiled(&small, &guide, 2, 1e-2, w.max(h) as usize);

    for (k, (&got, &want)) in tiled.as_raw().iter().zip(reference.as_raw().iter()).enumerate() {
        let x = k % w as usize;
        let y = k / w as usize;
        let delta = (got as i16 - want as i16).abs();
        let on_seam = (x != 0 && x.is_multiple_of(tile_edge))
            || (y != 0 && y.is_multiple_of(tile_edge));
        assert!(
            delta <= 1,
            "pixel ({x}, {y}), seam={on_seam}: tiled {got}, whole-frame {want}"
        );
    }
}

/// L14#7: the PRODUCTION tile geometry. The public entry hard-wires
/// GUIDED_REFINE_TILE_EDGE and every real caller crosses it (the GUI
/// refines at full decode resolution), yet the seam oracle above only
/// drives the parameterised internal — from its point of view the
/// shipped constant is dead code. The guide here exceeds the edge in
/// ONE axis (two real seams for ~100 K pixels; growing both axes would
/// square the cost for no added coverage), and the width is DERIVED
/// from the constant so the test keeps crossing two seams if the edge
/// ever changes. Same |delta| ≤ 1 tolerance: tiled and whole-frame
/// differ in f32 summation order, and bit-equality would be flaky
/// across targets.
#[test]
fn the_public_guided_refine_crosses_its_production_tile_seams_cleanly() {
    let w = (GUIDED_REFINE_TILE_EDGE * 2 + 52) as u32;
    let h = 48u32;
    // Slanted high-contrast edge sweeping through the first seam column,
    // plus per-channel dither — same generator family as the oracle test.
    let guide = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        let base: u8 = if x + y * 11 < w / 2 { 28 } else { 210 };
        image::Rgb([
            base,
            base.saturating_add(((x * 5 + y * 3) % 17) as u8),
            base.saturating_sub(((x * 2 + y * 7) % 13) as u8),
        ])
    }));
    let small = image::GrayImage::from_fn(17, 5, |x, y| {
        image::Luma([((x * 37 + y * 53 + (x * y % 7) * 19) % 256) as u8])
    });
    // eps matches the sole production caller (gui/masks.rs).
    let public = refine_mask_guided(&small, &guide, 2, 1e-4);
    let whole = refine_mask_guided_tiled(&small, &guide, 2, 1e-4, w.max(h) as usize);
    for (k, (&got, &want)) in public.as_raw().iter().zip(whole.as_raw().iter()).enumerate() {
        let x = k % w as usize;
        let y = k / w as usize;
        let delta = (got as i16 - want as i16).abs();
        let on_seam = x != 0 && x.is_multiple_of(GUIDED_REFINE_TILE_EDGE);
        assert!(
            delta <= 1,
            "pixel ({x}, {y}), production-seam-column={on_seam}: public {got}, \
             whole-frame {want}"
        );
    }
}

/// The public entry's OWN guards, unreachable through the internal fn
/// the seam tests drive: a hostile eps (NaN / negative / zero) is
/// floored to 1e-6 — not divided by (near-)zero variance and quantised
/// to black — and a 0×0 guide returns the mask unchanged.
#[test]
fn the_public_guided_refine_floors_a_hostile_eps() {
    let (w, h) = (64u32, 32u32);
    let guide = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, _| {
        if x < w / 2 { image::Rgb([20, 20, 20]) } else { image::Rgb([220, 220, 220]) }
    }));
    let small = image::GrayImage::from_fn(9, 5, |x, y| {
        image::Luma([((x * 41 + y * 59) % 256) as u8])
    });
    let floored = refine_mask_guided(&small, &guide, 2, 1e-6);
    assert!(
        floored.as_raw().iter().any(|&p| p > 0),
        "premise: the floored reference is not all black"
    );
    for bad in [f32::NAN, -1.0, 0.0] {
        let got = refine_mask_guided(&small, &guide, 2, bad);
        assert_eq!(
            got.as_raw(),
            floored.as_raw(),
            "eps {bad} must be floored to 1e-6, not quantise NaN to black"
        );
    }
    let empty = refine_mask_guided(&small, &DynamicImage::ImageRgb8(RgbImage::new(0, 0)), 2, 1e-4);
    assert_eq!(empty.as_raw(), small.as_raw(), "a 0x0 guide returns the mask unchanged");
}

/// A RAW with no colour matrix is DISCLOSED, not developed without its
/// profile in silence: `resolve_camera_profile` forwards this error
/// verbatim into its `camera profile "…" is not rendered: …` warning, so
/// the text has to name the missing thing. (That arm sits behind
/// `dcp::find`, which needs an installed profile pool; the error it
/// forwards is what can be pinned without one.)
///
/// MUTATION: answer an identity for an empty map, or reword the error.
#[test]
fn a_raw_without_a_colour_matrix_names_the_missing_matrix() {
    use rawler::cfa::{PlaneColor, CFA};
    use rawler::decoders::Camera;
    use rawler::rawimage::{BlackLevel, CFAConfig, RawImageData, RawPhotometricInterpretation, WhiteLevel};
    let raw = rawler::RawImage::new_with_data(
        Camera::new(),
        RawImageData::Integer(vec![512u16; 16]),
        4,
        4,
        1,
        [1.0, 1.0, 1.0, f32::NAN],
        RawPhotometricInterpretation::Cfa(CFAConfig::new(&CFA::new("RGGB"), &PlaneColor::new("RGB"))),
        Some(BlackLevel::new(&[0u16], 1, 1, 1)),
        Some(WhiteLevel::new(vec![4095])),
        false,
    );
    assert!(raw.color_matrix.is_empty(), "premise: a body rawler knows nothing about");
    let e = camera_matrix(&raw).unwrap_err().to_string();
    assert!(e.contains("no camera colour matrix"), "the refusal names the mechanism: {e}");
}

/// L04-1: a file-supplied camera matrix that cannot be inverted is
/// REFUSED with the measured determinant — never rendered into a
/// silently-black frame — while a real, well-conditioned matrix passes.
#[test]
fn singular_camera_matrix_is_refused_not_rendered_black() {
    let good = inv3(&rgb_to_xyz(SRGB_PRIM, D65_XY));
    assert!(
        validate_calibration(
            &good,
            [1.9, 1.0, 1.6, f32::NAN],
            ExportColorSpace::Srgb,
            Path::new("x.dng"),
        )
        .is_ok(),
        "a real matrix + real WB validates"
    );
    let mut twin = good;
    twin[1] = twin[0]; // two identical rows ⇒ det == 0 after row-norm
    let e = validate_calibration(&twin, [1.0, 1.0, 1.0, f32::NAN], ExportColorSpace::Srgb, Path::new("x.dng"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("determinant"), "the refusal quotes the measurement: {e}");
    assert!(e.contains("x.dng"), "the refusal names the file: {e}");
    let mut nan = good;
    nan[2][1] = f32::NAN;
    let e = validate_calibration(&nan, [1.0, 1.0, 1.0, f32::NAN], ExportColorSpace::Srgb, Path::new("x.dng"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("non-finite"), "{e}");
}

/// L04-1: a matrix row summing to zero used to be SILENTLY left
/// un-normalised by camera_to_space_matrix (the DNG white-preservation
/// rule quietly dropped) — the validator refuses it instead.
#[test]
fn degenerate_matrix_row_is_disclosed_not_silently_unnormalised() {
    let mut degen = inv3(&rgb_to_xyz(SRGB_PRIM, D65_XY));
    degen[0] = [0.5, -1.0, 0.5]; // sums to 0
    let e = validate_calibration(&degen, [1.0, 1.0, 1.0, f32::NAN], ExportColorSpace::Srgb, Path::new("x.dng"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("degenerate row"), "{e}");
    // Codex AL-review F3: a row can be healthy RAW but near-orthogonal
    // to the white-weighted PRODUCT the render actually inverts - the
    // validator must judge that product, not the raw rows alone.
    let m = rgb_to_xyz(SRGB_PRIM, D65_XY);
    let col_sums = [
        m[0][0] + m[0][1] + m[0][2],
        m[1][0] + m[1][1] + m[1][2],
        m[2][0] + m[2][1] + m[2][2],
    ];
    let mut ortho = inv3(&rgb_to_xyz(SRGB_PRIM, D65_XY));
    // row = [1, t, 0] with 1*col0 + t*col1 = 0  =>  raw sum 1+t != 0.
    let t = -col_sums[0] / col_sums[1];
    ortho[0] = [1.0, t, 0.0];
    let e = validate_calibration(&ortho, [1.0, 1.0, 1.0, f32::NAN], ExportColorSpace::Srgb, Path::new("x.dng"))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("degenerate row") || e.contains("determinant"),
        "a white-orthogonal row must refuse even with a healthy raw sum: {e}"
    );
}

/// L04-1: rawler turns a zero AsShotNeutral component into an INFINITE
/// wb coefficient (dng.rs builds 1/levels), which the old
/// `wb[0].is_nan()`-only guard let straight into the pixel chain — as
/// did a partial NaN and a negative. All refuse now; the documented
/// "unknown" convention (wb[0] NaN ⇒ neutral) still validates.
#[test]
fn zero_as_shot_neutral_becomes_an_infinite_coefficient_and_is_refused() {
    let good = inv3(&rgb_to_xyz(SRGB_PRIM, D65_XY));
    for bad in [
        [f32::INFINITY, 1.0, 1.0, f32::NAN], // the DNG 1/0 case
        [1.0, f32::NAN, 1.0, f32::NAN],      // partial NaN — invisible to a [0]-only check
        [1.0, -0.5, 1.0, f32::NAN],
        [1.0, 0.0, 1.0, f32::NAN],
    ] {
        let e = validate_calibration(&good, bad, ExportColorSpace::Srgb, Path::new("x.dng"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("AsShotNeutral"), "{bad:?} must refuse: {e}");
    }
    // A REAL (non-NaN) fourth coefficient is validated too (Codex F4):
    // rawler consumes it on 4-colour sensors.
    let e = validate_calibration(&good, [1.0, 1.0, 1.0, f32::INFINITY], ExportColorSpace::Srgb, Path::new("x.dng"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("fourth WB coefficient"), "{e}");
    assert!(validate_calibration(&good, [1.0, 1.0, 1.0, 1.2], ExportColorSpace::Srgb, Path::new("x.dng")).is_ok());
    assert_eq!(
        normalise_wb([f32::NAN, 9.0, 9.0, 9.0]),
        [1.0, 1.0, 1.0],
        "wb[0] NaN is rawler's documented UNKNOWN — neutral, not corrupt"
    );
    assert!(validate_calibration(&good, [f32::NAN; 4], ExportColorSpace::Srgb, Path::new("x")).is_ok());
    assert_eq!(
        normalise_wb([f32::INFINITY, 1.0, 1.0, f32::NAN]),
        [f32::INFINITY, 1.0, 1.0],
        "an infinite coefficient is NOT collapsed to unknown — it must reach the refusal"
    );
}

/// L04-1: WHY the refusal exists — the packer saturates silently. NaN
/// quantises to black, inf to white, and render_to_file would return Ok.
/// Pinned so a future refactor cannot re-open the silent path.
#[test]
fn nan_calibration_never_reaches_the_packer() {
    assert_eq!(to_u16(f32::NAN), 0, "NaN clamps to black");
    assert_eq!(to_u16(f32::INFINITY), 65535, "inf saturates to white");
    assert_eq!(to_u16(f32::NEG_INFINITY), 0);
}

/// L04-2: a CA-only profile (ca knots past 1, distortion off) used to
/// sample red OUTSIDE the frame along the whole border — the clamping
/// sampler smeared a radial plateau there (red[(0,mid)] == red[(1,mid)]
/// on any ramp), because the fill scale was hard-wired to 1.0 whenever
/// distortion was off. The composite fill zooms all channels in by the
/// overshoot, so every source sample stays inside the frame.
#[test]
fn ca_only_profile_never_samples_outside_the_frame() {
    use crate::recipe::LensProfile;
    let profile = LensProfile {
        ca_r: vec![1.02; 16],
        ca_b: vec![0.98; 16],
        ca_on: true,
        ..Default::default()
    };
    let ramp = DynamicImage::ImageRgb16(ImageBuffer::from_fn(200, 100, |x, _| {
        Rgb([(x as u16) * 300; 3])
    }));
    let out = apply_lens_geometry(&ramp, &profile, 0.0).to_rgb16();
    let a = out.get_pixel(0, 50).0[0];
    let b = out.get_pixel(1, 50).0[0];
    assert_ne!(
        a, b,
        "the border red plateau is gone — no clamped out-of-frame samples"
    );
    // …and the zoom leaves no unfilled pixels: a white frame stays white.
    let white =
        DynamicImage::ImageRgb8(RgbImage::from_pixel(201, 101, image::Rgb([255; 3])));
    let w = apply_lens_geometry(&white, &profile, 0.0).to_rgb16();
    let min = w.pixels().flat_map(|p| p.0).min().unwrap();
    assert!(min >= 65000, "unfilled pixels through the CA fill: min {min}");
}

/// L04-2: the fill is exactly 1.0 whenever no channel overshoots — real
/// profiles below unity (and CA-off entirely) cost nothing and stay
/// bit-identical: the green channel of a sub-unity-CA render equals the
/// CA-off render byte for byte (base LUT divided by exactly 1.0; the
/// two pixel branches use samplers with identical math).
#[test]
fn ca_fill_scale_is_identity_when_no_channel_overshoots() {
    use crate::recipe::LensProfile;
    let dims = (1200.0, 800.0);
    let sub = LensProfile {
        ca_r: vec![0.999; 16],
        ca_b: vec![0.998; 16],
        ca_on: true,
        ..Default::default()
    };
    assert_eq!(geometry_fill_scale(&sub, 0.0, dims), 1.0);
    assert_eq!(
        geometry_fill_scale(&LensProfile::default(), 25.0, dims),
        1.0,
        "CA off is always exactly 1.0 — the manual path never pays"
    );
    let dist: Vec<f32> =
        (0..16).map(|i| 1.0008 - 0.02 * (i as f32 / 15.0).powi(2)).collect();
    let with_ca = LensProfile {
        distortion: dist.clone(),
        distortion_on: true,
        ca_r: vec![0.999; 16],
        ca_b: vec![0.999; 16],
        ca_on: true,
        ..Default::default()
    };
    let no_ca =
        LensProfile { distortion: dist, distortion_on: true, ..Default::default() };
    let ramp = DynamicImage::ImageRgb16(ImageBuffer::from_fn(160, 90, |x, y| {
        Rgb([(x as u16) * 300, (x as u16) * 300 + (y as u16), (y as u16) * 500])
    }));
    let a = apply_lens_geometry(&ramp, &with_ca, 0.0).to_rgb16();
    let b = apply_lens_geometry(&ramp, &no_ca, 0.0).to_rgb16();
    assert!(
        a.pixels().zip(b.pixels()).all(|(p, q)| p.0[1] == q.0[1]),
        "green must be byte-identical when the fill is exactly 1"
    );
}

/// R25 B3: the manual CA pair really moves the red channel's sampling
/// radius — and moves NOTHING else when it asks for a shrink.
///
/// A NEGATIVE ca_r is the sharp case: every channel factor lands at or
/// below 1, so [`geometry_fill_scale`] is exactly 1.0 and green and blue
/// come out byte for byte identical to the CA-off render, leaving red as
/// the only thing that moved. (The positive direction cannot make that
/// claim and must not pretend to: an overshooting channel zooms the whole
/// frame through the composite fill, which is the L04-2 contract, and it
/// is asserted below rather than dodged.)
#[test]
fn manual_ca_shifts_the_red_channel_radius() {
    // Concentric rings: a pattern that is a function of RADIUS, so a
    // radial rescale of one channel is visible and a translation-only bug
    // could not fake it. All three channels identical at the source.
    let target = DynamicImage::ImageRgb16(ImageBuffer::from_fn(161, 161, |x, y| {
        let (dx, dy) = (x as f32 - 80.0, y as f32 - 80.0);
        let r = (dx * dx + dy * dy).sqrt();
        let v = (((r * 0.7).sin() * 0.5 + 0.5) * 60000.0) as u16;
        Rgb([v; 3])
    }));
    let off = EditRecipe::default();
    let shrink = EditRecipe { ca_r: -100.0, ..Default::default() };
    let grow = EditRecipe { ca_r: 100.0, ..Default::default() };

    // The composed profile is what carries it — a photo with no in-camera
    // CA data of its own still gets one.
    assert!(!geometry_profile(&off).geometry_active(), "premise: a rest recipe adds nothing");
    assert!(geometry_profile(&shrink).geometry_active(), "the manual pair alone is geometry");

    let base = apply_lens_geometry(&target, &geometry_profile(&off), 0.0).to_rgb16();
    let out = apply_lens_geometry(&target, &geometry_profile(&shrink), 0.0).to_rgb16();
    assert!(
        out.pixels().zip(base.pixels()).all(|(p, q)| p.0[1] == q.0[1] && p.0[2] == q.0[2]),
        "a shrinking manual CA must leave green and blue byte-identical"
    );
    assert!(
        out.pixels().zip(base.pixels()).any(|(p, q)| p.0[0] != q.0[0]),
        "…and it must actually move red"
    );
    // The DIRECTION, at one named pixel: ca_r < 0 means red samples at a
    // SMALLER radius, so the far-off-centre red is the source from nearer
    // the middle. SUB-PIXEL by construction — ±100 is 0.2 % of the radius
    // and no test frame makes that a whole pixel — so the expectation is
    // the source's own bilinear value at the shrunk coordinate, which on
    // the centre row (dy = 0) is a plain lerp along x.
    let src = target.to_rgb16();
    let f = 1.0 + (-100.0) * MANUAL_CA_PER_UNIT;
    let (x, y) = (158u32, 80u32);
    let sx = 80.0 + ((x as f32) - 80.0) * f;
    let (i0, frac) = (sx.floor() as u32, sx - sx.floor());
    let want = src.get_pixel(i0, y).0[0] as f32 * (1.0 - frac)
        + src.get_pixel(i0 + 1, y).0[0] as f32 * frac;
    let got = out.get_pixel(x, y).0[0] as f32;
    assert!(
        (got - want).abs() < 64.0,
        "red at x={x} should be the source at the shrunk radius {sx}: got {got}, want {want}"
    );
    // …and that really is a MOVE, not a rounding: the unshifted source
    // pixel is far away on this ring pattern.
    let unshifted = src.get_pixel(x, y).0[0] as f32;
    assert!(
        (got - unshifted).abs() > 1000.0,
        "the probe pixel must sit on a steep part of the ring pattern"
    );

    // The other direction is the documented frame zoom, not a silent one:
    // an overshooting channel makes `geometry_moves_frame` true, which is
    // what keeps every coordinate map in step (C2).
    assert!(
        geometry_moves_frame(&geometry_profile(&grow), 0.0),
        "a magnifying manual CA zooms the frame through the composite fill"
    );
    assert!(
        !geometry_moves_frame(&geometry_profile(&shrink), 0.0),
        "…and a shrinking one does not move the shared frame at all"
    );
}

/// R25 B3: the pair at rest costs NOTHING — no allocation, no engine
/// change, and a render byte for byte what v0.30 produced.
#[test]
fn ca_zero_is_bit_identical_to_no_ca() {
    use crate::recipe::LensProfile;
    let profile = LensProfile {
        distortion: (0..16).map(|i| 1.0008 - 0.02 * (i as f32 / 15.0).powi(2)).collect(),
        distortion_on: true,
        ca_r: vec![0.999; 16],
        ca_b: vec![1.001; 16],
        ca_on: true,
        ..Default::default()
    };
    let r = EditRecipe { lens_profile: profile.clone(), ..Default::default() };
    assert!(
        matches!(geometry_profile(&r), std::borrow::Cow::Borrowed(_)),
        "a recipe with ca_r = ca_b = 0 must borrow the in-camera profile unchanged"
    );
    assert_eq!(*geometry_profile(&r), profile, "…and it is that profile, not a copy of one");
    let ramp = DynamicImage::ImageRgb16(ImageBuffer::from_fn(160, 90, |x, y| {
        Rgb([(x as u16) * 300, (x as u16) * 300 + (y as u16), (y as u16) * 500])
    }));
    let a = apply_lens_geometry(&ramp, &geometry_profile(&r), 0.0).to_rgb16();
    let b = apply_lens_geometry(&ramp, &profile, 0.0).to_rgb16();
    assert!(a.pixels().zip(b.pixels()).all(|(p, q)| p.0 == q.0), "the render must not move");
    // A profile whose CA the user switched OFF stays off: the manual pair
    // stands alone rather than scaling knots nobody asked for.
    let off = EditRecipe {
        lens_profile: LensProfile { ca_on: false, ..profile },
        ca_r: -50.0,
        ..Default::default()
    };
    let composed = geometry_profile(&off);
    assert_eq!(composed.ca_r.len(), 1, "the disabled profile knots must not participate");
    assert!((composed.ca_r[0] - (1.0 + -50.0 * MANUAL_CA_PER_UNIT)).abs() < 1e-9);
    assert_eq!(composed.ca_b, vec![1.0], "the untouched axis is exactly neutral");
}

/// **Every tier that renders nothing renders nothing** — re-derived from
/// the registry so a row that changes tier without gaining an engine stage
/// fails here.
///
/// R25 B3 wrote this over `Tier::CarriedOnly` alone, when that tier had
/// fifteen members. v1.5.0 emptied it — Track F: everything this app can
/// set now renders — so the sieve is the PROPERTY, `!tier.renders()`,
/// rather than the one tier that happened to have the members, and
/// `passthrough` is what keeps it from being a test over nothing. The
/// opposite direction, that the twenty-four promoted rows really do render,
/// is `advisor::catalogue::tests::
/// nothing_is_carried_any_more_and_every_row_that_left_renders` plus each
/// operator's own module test.
#[test]
fn a_tier_that_renders_nothing_renders_nothing() {
    use crate::advisor::catalogue::{global_value, Shape, RECIPE_CONTROLS};
    let img = DynamicImage::ImageRgb16(ImageBuffer::from_fn(64, 48, |x, y| {
        Rgb([(x as u16) * 900, (y as u16) * 1200, ((x + y) as u16) * 500])
    }));
    let neutral_recipe = EditRecipe::default();
    let neutral = develop_preview(&img, &neutral_recipe).to_rgb16();
    let mut probed = 0usize;
    for c in RECIPE_CONTROLS.iter().filter(|c| c.tier.is_some_and(|t| !t.renders())) {
        // Through serde, so a renamed field cannot slip past — and BY
        // SHAPE, because B3 put a flag on this tier and B4 a whole map.
        let mut json = serde_json::to_value(&neutral_recipe).expect("recipe serialises");
        json[c.name] = match c.shape {
            Shape::Bool => serde_json::json!(true),
            // `passthrough` is a MAP of crs keys nobody interprets, so the
            // probe has to be a key Lightroom really writes — an invented
            // one would be dropped and the probe would move nothing, which
            // this test would then read as inertness.
            Shape::EngineCarrier => serde_json::json!({ "PerspectiveVertical": "-35" }),
            _ => serde_json::json!(c.range.map(|(_, hi)| hi).unwrap_or(1.0)),
        };
        let mut r: EditRecipe = serde_json::from_value(json)
            .unwrap_or_else(|e| panic!("{}: probe value rejected: {e}", c.name));
        r.clamp();
        assert_ne!(
            global_value(&r, c.name),
            global_value(&neutral_recipe, c.name),
            "{}: the probe must actually move the control",
            c.name
        );
        let out = develop_preview(&img, &r).to_rgb16();
        assert!(
            out.pixels().zip(neutral.pixels()).all(|(p, q)| p.0 == q.0),
            "{}: a non-rendering tier moved a pixel — then it is not carried, it renders",
            c.name
        );
        // The GEOMETRIC stage runs after `develop_preview`, so inertness
        // there needs its own word: a carried control must not reach the
        // composed lens profile either (the manual CA pair is the only
        // thing on that path, and it is `Rendered`).
        assert_eq!(
            *geometry_profile(&r),
            *geometry_profile(&neutral_recipe),
            "{}: a non-rendering tier changed the composed lens geometry",
            c.name
        );
        // …nor the stage AFTER the geometry, which v1.5.0 added: an
        // operator that reached only the post-crop finish would have passed
        // both assertions above and still changed the delivered frame.
        let tail = |r: &EditRecipe| {
            frame_and_finish(
                DynamicImage::ImageRgb16(neutral.clone()),
                r,
                &geometry_profile(r),
                FilmScale::NATIVE,
                CropPolicy::Cut,
            )
            .to_rgb16()
        };
        assert!(
            tail(&r).pixels().zip(tail(&neutral_recipe).pixels()).all(|(p, q)| p.0 == q.0),
            "{}: a non-rendering tier moved a pixel in the finishing pass",
            c.name
        );
        probed += 1;
    }
    // The premise, and it is now a NAMED one: with `CarriedOnly` empty,
    // `passthrough` is the only row this sieve can reach, so an unnamed
    // count would slide to zero the day someone retired it.
    assert_eq!(
        probed, 1,
        "premise: B4's `passthrough` is the last non-rendering row — Track F emptied \
         `CarriedOnly`, so a second one here is a decision and a zero is a hole"
    );
}

/// L04-2: the C2 coordinate contract — the GUI's normalised maps carry
/// the SAME composite fill as the render, so masks/dropper/clone points
/// stay on the pixels; the fill-adjusted forward/inverse pair still
/// round-trips; and a CA-only profile no longer short-circuits the
/// shared map to the manual path.
#[test]
fn ca_fill_keeps_the_gui_map_in_step_with_the_render() {
    use crate::recipe::LensProfile;
    let profile = LensProfile {
        distortion: (0..16).map(|i| 1.0008 - 0.053 * (i as f32 / 15.0).powi(2)).collect(),
        ca_r: vec![1.004; 16],
        ca_b: vec![0.997; 16],
        distortion_on: true,
        ca_on: true,
        ..Default::default()
    };
    let dims = (1200.0, 800.0);
    let fill = geometry_fill_scale(&profile, 0.0, dims);
    assert!(fill > 1.0, "premise: this profile's red channel overshoots");
    // The normalised map's radial factor at an edge point equals the
    // render's green factor (base / fill) at the same rn.
    let (nx, ny) = (0.98, 0.5);
    let (ox, _) = lens_geom_norm(nx, ny, dims, &profile, 0.0);
    let f_norm = (ox - 0.5) / (nx - 0.5);
    let (w, h) = dims;
    let rn = ((nx - 0.5) * w).abs() / (0.5 * (w * w + h * h).sqrt());
    let s_p = profile_fill_scale(&profile.distortion, dims);
    let expect = lens_geom_factor(rn, &profile.distortion, s_p, 0.0, 1.0) / fill;
    assert!(
        (f_norm - expect).abs() < 1e-4,
        "the GUI map factor {f_norm} drifted from the render's {expect}"
    );
    // Forward/inverse still round-trip through the fill-adjusted pair.
    for (px, py) in [(0.1, 0.2), (0.9, 0.85), (0.5, 0.05)] {
        let (ax, ay) = lens_geom_norm(px, py, dims, &profile, 12.0);
        let (bx, by) = lens_ungeom_norm(ax, ay, dims, &profile, 12.0);
        assert!(
            (bx - px).abs() < 2e-3 && (by - py).abs() < 2e-3,
            "roundtrip ({px},{py}) → ({bx},{by})"
        );
    }
    // Distortion OFF + overshooting CA: the shared map must move (the
    // old early-return to the manual path skipped the fill entirely).
    let ca_only = LensProfile {
        ca_r: vec![1.02; 16],
        ca_b: vec![0.98; 16],
        ca_on: true,
        ..Default::default()
    };
    let (mx, _) = lens_geom_norm(0.9, 0.5, dims, &ca_only, 0.0);
    assert!(
        (mx - 0.9).abs() > 1e-4,
        "the composite fill moves the shared map even with distortion off"
    );
    let (fx, fy) = lens_geom_norm(0.8, 0.3, dims, &ca_only, 0.0);
    let (ux, uy) = lens_ungeom_norm(fx, fy, dims, &ca_only, 0.0);
    assert!(
        (ux - 0.8).abs() < 2e-3 && (uy - 0.3).abs() < 2e-3,
        "CA-only roundtrip ({ux},{uy})"
    );
}

/// 阶段4: the export DEPTH is an ExportOpts setting, not an extension
/// property — a .png/.tif carries 16 bits by default and 8 on request,
/// and the staged publish keeps the contract for both.
#[test]
fn export_depth_follows_the_option_not_the_extension() {
    let dir = std::env::temp_dir().join(format!(
        "autoshade-export-depth-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let src_dir = dir.join("library");
    let out_dir = dir.join("exports");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&out_dir).unwrap();
    let src = src_dir.join("in.png");
    RgbImage::from_fn(12, 8, |x, y| image::Rgb([x as u8 * 20, y as u8 * 30, 90]))
        .save(&src)
        .unwrap();
    let recipe = EditRecipe::default();
    for (name, eight, want8) in [
        ("d16.png", false, false),
        ("d8.png", true, true),
        ("d16.tif", false, false),
        ("d8.tif", true, true),
    ] {
        let out = out_dir.join(name);
        let opts = ExportOpts { eight_bit: eight, ..Default::default() };
        render_to_file(&src, &recipe, &out, None, Some(&opts), crate::diag::stderr()).unwrap();
        let img = image::open(&out).unwrap();
        let got8 = matches!(img.color(), image::ColorType::Rgb8 | image::ColorType::Rgba8);
        assert_eq!(
            got8, want8,
            "{name}: eight_bit={eight} must decide the stored depth, got {:?}",
            img.color()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// An AI mask with no recomputed alpha must SKIP its adjustment, not
/// render it at weight 0.
///
/// The distinction is not academic — it is the difference between "this
/// mask does nothing" and "this mask does everything". `LocalAdjustment`
/// composes coverage with `inverted` as `1 - w`, so a zero-coverage mask
/// under `inverted: true` applies the edit to the ENTIRE frame. That is
/// exactly the silent-zero failure the AI arm is not allowed to have, and
/// `is_raster_backed` is what separates "needs pixels and has none" from
/// "needs no pixels".
///
/// MUTATION-LINED. Verified red by reverting the two `is_raster_backed`
/// call sites to `matches!(…, MaskGeometry::Bitmap { .. })` (transcript in
/// the batch report): the inverted arm's assertion fails because the
/// unresolved AI mask brightens the whole frame.
#[test]
fn an_unresolved_ai_mask_skips_its_adjustment_instead_of_covering_the_frame() {
    let ai = |inverted: bool| crate::recipe::LocalAdjustment {
        name: "Sky".into(),
        inverted,
        exposure_ev: 3.0,
        mask: MaskGeometry::AiMask {
            name: "Sky 1".into(),
            subtype: 2,
            ref_x: 0.5,
            ref_y: 0.3,
            blend_mode: 0,
            value: 1.0,
            inverted: false,
            mask_version: 1,
            provenance: Vec::new(),
            gesture: Vec::new(),
            // NOT resolved: the segmenter has not run, or declined.
            raster: None,
        },
        ..Default::default()
    };
    // The two questions the weight loop asks, answered directly — the same
    // pair `apply_masks` and `mask_coverage` both consult.
    let g = &ai(false).mask;
    assert!(is_raster_backed(g), "an AI mask draws from a raster");
    assert!(geometry_raster_path(g).is_none(), "…and it has none yet");
    // With no bitmap, the weight is 0 everywhere — which is exactly why the
    // caller must skip rather than invert it.
    assert_eq!(mask_weight(g, 0.5, 0.3, None), 0.0);
    assert_eq!(mask_weight(g, 0.1, 0.9, None), 0.0);

    // The visible consequence, through the public coverage preview: an
    // unresolved AI mask advertises NO coverage in either polarity. Under
    // the old Bitmap-only test the inverted one would have advertised the
    // whole frame.
    let reference = DynamicImage::ImageRgb8(image::RgbImage::new(8, 8));
    for inverted in [false, true] {
        let cov = mask_coverage(&ai(inverted), &reference, MaskFrame::AsRendered);
        let lit = cov.pixels().filter(|p| p.0[0] > 0).count();
        assert_eq!(
            lit, 0,
            "an unresolved AI mask must advertise nothing (inverted={inverted})"
        );
    }

    // And once an alpha EXISTS the geometry samples it like any raster.
    let dir = std::env::temp_dir().join(format!("autoshade-ai-render-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("alpha.png");
    let mut img = image::GrayImage::new(4, 4);
    for px in img.pixels_mut() {
        px.0[0] = 255;
    }
    img.save(&p).unwrap();
    let mut resolved = ai(false);
    if let MaskGeometry::AiMask { raster, .. } = &mut resolved.mask {
        *raster = Some(p.to_string_lossy().into_owned());
    }
    assert_eq!(
        geometry_raster_path(&resolved.mask),
        Some(p.to_string_lossy().as_ref()),
        "the resolved alpha is the geometry's raster"
    );
    let bmp = load_mask_bitmap(&resolved.mask, &crate::diag::pixels()).expect("the alpha must load");
    assert_eq!(mask_weight(&resolved.mask, 0.5, 0.5, Some(&bmp)), 1.0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A reverse-fit ZONE renders through its Select Sky component EXACTLY as
/// it rendered through the `MaskGeometry::Bitmap` that carrier replaced —
/// byte for byte, upright and inverted.
///
/// The change was made for the SIDECAR (classic XMP has no encoding for a
/// raster mask, so both zone corrections used to be skipped with a named
/// `MaskLossReason::Bitmap`), and the one thing it must not do is move a
/// pixel of what the photographer already sees. Both carriers hold the
/// same claimed PNG and `mask_weight` samples them through the same
/// `sample_gray_norm`, so equality here is the proof that nothing about
/// the correction changed except the form it is written down in.
///
/// The INVERTED arm is THE TRIPWIRE FOR THE NET BEING APPLIED EXACTLY
/// ONCE. A zone spells its inversion in ONE home — `LocalAdjustment::
/// inverted`, the flag the weight loop reads — and the Select Sky
/// component's own bit stays `false`, so
/// [`crate::recipe::LocalAdjustment::net_inverted`] is `true` once and the
/// render inverts once. The AI arm of `mask_weight` honours the geometry's
/// bit now (it read no inversion at all before this change); if the zone
/// ever went back to spelling the fact in both homes, the land zone would
/// invert TWICE, cover the sky it excludes, and this assertion is what
/// says so.
///
/// MUTATION: give `select_sky` below the same `inverted` the adjustment
/// carries — the two-home spelling this replaced — and the `inverted` pass
/// fails.
#[test]
fn a_zone_ai_mask_renders_exactly_like_the_bitmap_it_replaced() {
    let dir =
        std::env::temp_dir().join(format!("autoshade-zone-ai-render-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("mask-zone-sky.png");
    // A real partition with a soft edge, not a flat mask: a flat one
    // renders identically under any sampling and could not witness a
    // coverage difference between the two carriers.
    image::GrayImage::from_fn(16, 16, |_, y| {
        image::Luma([match y {
            0..=5 => 255,
            6 => 160,
            7 => 64,
            _ => 0,
        }])
    })
    .save(&p)
    .unwrap();
    let path = p.to_string_lossy().into_owned();
    let src = DynamicImage::ImageRgb8(image::RgbImage::from_fn(24, 16, |x, y| {
        image::Rgb([40 + (x * 7 % 180) as u8, 60 + (y * 5 % 160) as u8, 120])
    }));
    let plain = develop_preview(&src, &EditRecipe::default()).to_rgb8().into_raw();
    for (inverted, role) in [
        (false, crate::recipe::MaskRole::ZoneSky),
        (true, crate::recipe::MaskRole::ZoneLand),
    ] {
        let zone = |mask: MaskGeometry| EditRecipe {
            masks: vec![crate::recipe::LocalAdjustment {
                mask,
                role,
                inverted,
                exposure_ev: -0.7,
                saturation: 18.0,
                color_gains: Some([1.12, 0.98, 0.87]),
                ..Default::default()
            }],
            ..Default::default()
        };
        let was = develop_preview(&src, &zone(MaskGeometry::Bitmap { path: path.clone() }))
            .to_rgb8()
            .into_raw();
        let now = develop_preview(
            &src,
            // `false`, not `inverted`: the correction's flag above is the
            // zone's ONE home for it (`fit_zoned`'s `land_attachment`).
            &zone(MaskGeometry::select_sky(0.5, 0.2, false, path.clone())),
        )
        .to_rgb8()
        .into_raw();
        assert_eq!(
            was, now,
            "inverted={inverted}: the Select Sky carrier must render the bitmap's own pixels"
        );
        // Premise: the correction is not a no-op, so the equality above is
        // not two identical copies of the untouched frame.
        assert_ne!(
            plain, now,
            "inverted={inverted}: the zone must actually change the render"
        );
    }
    // …and the two polarities are not each other, which is what makes the
    // inverted arm a real second case.
    let arm = |inverted: bool| {
        develop_preview(
            &src,
            &EditRecipe {
                masks: vec![crate::recipe::LocalAdjustment {
                    mask: MaskGeometry::select_sky(0.5, 0.2, false, path.clone()),
                    role: crate::recipe::MaskRole::ZoneSky,
                    inverted,
                    exposure_ev: -0.7,
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .to_rgb8()
        .into_raw()
    };
    assert_ne!(arm(false), arm(true), "the inversion must reach the pixels");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Native Lightroom inversion remains a component-owned bit, and must
/// render the complement of a graded alpha. AutoShade's optional editing
/// metadata instead restores the authored inversion home. A native edit
/// that contradicts that metadata wins; neither path may invert twice.
///
/// The native-only pair still differs in exactly MaskInverted. The
/// authored pair also differs in ash:Inverted, deliberately, and both
/// inconsistent pairs test that stale editing intent cannot veto CRS.
/// Dropping the AI geometry's own bit or applying either bit twice must
/// still fail the exact coverage assertions below.
#[test]
fn ai_mask_import_preserves_authored_inversion_and_native_edits_win() {
    let dir =
        std::env::temp_dir().join(format!("autoshade-ai-import-inv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("alpha.png");
    // A GRADED alpha: a flat one cannot tell `1 - w` from a constant.
    image::GrayImage::from_fn(8, 8, |x, _| image::Luma([(x * 32) as u8])).save(&p).unwrap();
    let path = p.to_string_lossy().into_owned();

    // The export, from the zone's own one-home shape: the correction
    // carries the flag, the component carries `false`, and the sidecar
    // gets their net on the component — which is the only place Lightroom
    // has for it.
    let recipe = |inverted: bool| EditRecipe {
        masks: vec![crate::recipe::LocalAdjustment {
            mask: MaskGeometry::select_sky(0.5, 0.25, false, path.clone()),
            role: crate::recipe::MaskRole::ZoneLand,
            inverted,
            exposure_ev: -0.5,
            ..Default::default()
        }],
        ..Default::default()
    };
    // The PROJECTION (payload-free, v1.3.1): the native bit and the `ash`
    // intent are the two channels this test is about. The payload is a
    // third, and the whole documents below put it through the same rows.
    let exported = |inverted: bool| crate::xmp::bare_document(&recipe(inverted), None);
    let authored_inv = exported(true);
    let authored_up = exported(false);
    assert!(
        authored_inv.contains(r#"crs:What="Mask/Image""#)
            && authored_inv.contains(r#"crs:MaskInverted="true""#),
        "premise: the fixture must BE an inverted Lightroom AI mask:\n{authored_inv}"
    );
    let without_intent = |mut text: String| {
        while let Some(start) = text.find(" ash:") {
            let value = start + text[start..].find("=\"").unwrap() + 2;
            let end = value + text[value..].find('"').unwrap() + 1;
            text.replace_range(start..end, "");
        }
        text
    };
    let inv_doc = without_intent(authored_inv.clone());
    let up_doc = inv_doc.replace(r#"crs:MaskInverted="true""#, r#"crs:MaskInverted="false""#);
    assert_eq!(up_doc, without_intent(authored_up.clone()),
        "the native-only exports differ in the inversion attribute and nothing else");
    assert_eq!(
        authored_inv.replace(r#"crs:MaskInverted="true""#, r#"crs:MaskInverted="false""#)
            .replace(r#"ash:Inverted="true""#, r#"ash:Inverted="false""#),
        authored_up,
        "the authored exports also record the inversion home"
    );
    let edited_up = authored_inv.replace(r#"crs:MaskInverted="true""#, r#"crs:MaskInverted="false""#);
    let edited_inv = authored_up.replace(r#"crs:MaskInverted="false""#, r#"crs:MaskInverted="true""#);
    // The whole documents (v1.3.1): the payload restores the authored home
    // when the net Lightroom hands back is the net that was written — the
    // intent still there, or stripped the way Lightroom strips it — and
    // yields to a native edit of the net, in Lightroom's home.
    let whole = |inverted: bool| crate::xmp::recipe_to_xmp(&recipe(inverted));
    let (whole_inv, whole_up) = (whole(true), whole(false));
    let (rewritten_inv, rewritten_up) =
        (without_intent(whole_inv.clone()), without_intent(whole_up.clone()));
    let rewritten_edited_up =
        rewritten_inv.replace(r#"crs:MaskInverted="true""#, r#"crs:MaskInverted="false""#);
    let rewritten_edited_inv =
        rewritten_up.replace(r#"crs:MaskInverted="false""#, r#"crs:MaskInverted="true""#);
    let bmp = image::open(&p).unwrap().to_luma8();
    for (attr, text, want_inverted, want_whole) in [
        ("native inverted", &inv_doc, true, false),
        ("native upright", &up_doc, false, false),
        ("authored inverted", &authored_inv, true, true),
        ("authored upright", &authored_up, false, false),
        ("native edit upright", &edited_up, false, false),
        ("native edit inverted", &edited_inv, true, false),
        ("payload inverted", &whole_inv, true, true),
        ("payload upright", &whole_up, false, false),
        ("payload, intent stripped", &rewritten_inv, true, true),
        ("payload, native edit upright", &rewritten_edited_up, false, false),
        ("payload, native edit inverted", &rewritten_edited_inv, true, false),
    ]
    {
        let back = crate::xmp::xmp_to_recipe(text);
        assert_eq!(back.masks.len(), 1, "{attr}: {:?}", back.masks);
        let m = &back.masks[0];
        let want_component = want_inverted ^ want_whole;
        assert_eq!(m.inverted, want_whole, "{attr}: preserve only consistent authored intent");
        assert_eq!(m.mask.own_inverted(), want_component, "{attr}: the native fallback owns its bit");
        assert_eq!(m.net_inverted(), want_inverted, "{attr}: the native net wins");
        // THE RENDER. `raster` is `None` on import — the sidecar carries
        // the intent, never Adobe's pixels — so point it at the alpha this
        // engine would recompute and sample the geometry directly.
        let mut g = m.mask.clone();
        // The SLOT, not `geometry_raster_path_mut`: that helper repoints a
        // raster a mask already has, and an imported AI mask has none —
        // which is the state this is standing in for the segmenter to fill.
        let MaskGeometry::AiMask { raster, .. } = &mut g else {
            panic!("{attr}: expected the Select Sky component back, got {g:?}");
        };
        *raster = Some(path.clone());
        for (nx, ny) in [(0.1f32, 0.5f32), (0.4, 0.5), (0.9, 0.5)] {
            let upright = sample_gray_norm(&bmp, nx, ny);
            let want = if want_inverted { 1.0 - upright } else { upright };
            assert_eq!(mask_weight(&g, nx, ny, Some(&bmp)),
                if want_component { 1.0 - upright } else { upright },
                "{attr}: the geometry honours its own bit exactly once");
            let composed = combined_mask_weight(m, nx, ny, Some(&bmp), &[], None, (8.0, 8.0));
            assert_eq!(
                if m.inverted { 1.0 - composed } else { composed },
                want,
                "{attr}: at ({nx}, {ny}) the weight must be the {} alpha",
                if want_inverted { "complement of the" } else { "upright" }
            );
        }
        // Premise for the pair: the alpha is not its own complement here.
        assert_ne!(
            sample_gray_norm(&bmp, 0.1, 0.5),
            1.0 - sample_gray_norm(&bmp, 0.1, 0.5),
            "the fixture must be able to witness an inversion"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The `coord_era` migration turns an AI mask's reference point and DROPS
/// its cached alpha — the raster was segmented in the old frame, so
/// rotating it is a re-render, not a coordinate migration.
///
/// MUTATION: leave `*raster` alone in `orient_recipe_coords`' AiMask arm
/// and the second assert fails — the mask would then render the OLD
/// frame's selection over the turned pixels.
#[test]
fn turning_the_frame_moves_the_ai_click_and_invalidates_its_cached_alpha() {
    let mut r = EditRecipe {
        coord_era: 0, // the LEGACY era — what the migration acts on
        masks: vec![crate::recipe::LocalAdjustment {
            exposure_ev: 1.0,
            mask: MaskGeometry::AiMask {
                name: "Sky 1".into(),
                subtype: 2,
                ref_x: 0.25,
                ref_y: 0.10,
                blend_mode: 0,
                value: 1.0,
                inverted: false,
                mask_version: 1,
                provenance: Vec::new(),
                gesture: Vec::new(),
                raster: Some("stale.png".into()),
            },
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(
        recipe_has_frame_coords(&r),
        "an AI mask's reference point IS a frame coordinate the migration moves"
    );
    let turned = orient_recipe_coords(&mut r, rawler::Orientation::Rotate90, probe_frame());
    assert!(turned, "the migration ran");
    let MaskGeometry::AiMask { ref_x, ref_y, raster, .. } = &r.masks[0].mask else {
        panic!("the geometry must survive the migration");
    };
    assert_ne!((*ref_x, *ref_y), (0.25, 0.10), "the click moved with the frame");
    assert!(raster.is_none(), "the alpha from the OLD frame must not be reused");
}

/// …and the ZONE masks are the one exception, because their alpha is not a
/// cache of anything.
///
/// The zoned reverse-fit renders that PNG itself, claims it under a unique
/// name and measures the zone's exposure, gains and saturation against it;
/// no re-segmentation reproduces it from the recipe. Dropping it on a
/// rotate would leave both zone corrections inert until a model run — and
/// inert forever on a machine with no segmentation sidecar, i.e. the
/// photographer's edit silently gone. So it is KEPT here and turned by
/// `pipeline::rotate_recipe` phase 1 with the `Bitmap` rasters
/// (`LocalAdjustment::turnable_raster_paths_mut`), and by the `coord_era`
/// migration since v1.6.0 (`pipeline::migrate_raster_file`).
///
/// MUTATION: drop the `owned_alpha` guard in `orient_recipe_coords`' AiMask
/// arm and the `raster` assertion fails.
#[test]
fn turning_the_frame_keeps_a_zone_masks_own_alpha_and_turns_it() {
    let mut r = EditRecipe {
        coord_era: 0,
        masks: vec![crate::recipe::LocalAdjustment {
            exposure_ev: 1.0,
            role: crate::recipe::MaskRole::ZoneSky,
            mask: MaskGeometry::select_sky(0.25, 0.10, false, "mask-zone-sky.png".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert_eq!(
        r.masks[0].turnable_raster_paths_mut(),
        vec![&mut "mask-zone-sky.png".to_string()],
        "and it is in the walk `rotate_recipe` turns"
    );
    assert!(orient_recipe_coords(&mut r, rawler::Orientation::Rotate90, probe_frame()));
    let MaskGeometry::AiMask { ref_x, ref_y, raster, .. } = &r.masks[0].mask else {
        panic!("the geometry must survive the migration");
    };
    assert_ne!((*ref_x, *ref_y), (0.25, 0.10), "the click still moves with the frame");
    assert_eq!(
        raster.as_deref(),
        Some("mask-zone-sky.png"),
        "the fit's own alpha is kept for phase 1 to turn, never dropped"
    );
}

// ---- R28 Batch-1 1a: the CFA-geometry demosaic ------------------------

/// The X-S10's 6×6 X-Trans tile, read back from the zoo RAF's OWN
/// `XTransLayout` (0x0131) record rather than from rawler's camera DB —
/// the RAF decoder prefers the file's copy (`decoders/raf.rs:257-263`),
/// and for this body the two turned out identical (measured 2026-08-20,
/// `AUTOSHADE_RAW_ZOO` probe, alongside `active_area = 6252×4176 @ (0,5)`
/// and `crop_area = 6240×4160 @ (6,13)`).
const XTRANS_XS10: &str = "GGRGGBGGBGGRBRGRBGGGBGGRGGRGGBRBGBRG";

/// 20 green / 8 red / 8 blue in four 2×2 all-green blocks plus four
/// isolated greens — the structure every claim below rests on.
#[test]
fn the_x_trans_tile_is_four_green_blocks_and_four_isolated_greens() {
    use rawler::cfa::{CFA, CFA_COLOR_B, CFA_COLOR_G, CFA_COLOR_R};
    let cfa = CFA::new(XTRANS_XS10);
    assert_eq!((cfa.width, cfa.height), (6, 6));
    let count = |ch: usize| (0..36).filter(|i| cfa.color_at(i / 6, i % 6) == ch).count();
    assert_eq!((count(CFA_COLOR_R), count(CFA_COLOR_G), count(CFA_COLOR_B)), (8, 20, 8));
    // A green whose right AND down neighbours are both green is the
    // top-left corner of a 2×2 block; there are four of them.
    let blocks = (0..36)
        .filter(|i| {
            let (r, c) = (i / 6, i % 6);
            cfa.color_at(r, c) == CFA_COLOR_G
                && cfa.color_at(r, c + 1) == CFA_COLOR_G
                && cfa.color_at(r + 1, c) == CFA_COLOR_G
        })
        .count();
    assert_eq!(blocks, 4, "X-Trans is defined by its four 2×2 green blocks per tile");
}

/// WHY this file has its own demosaic, pinned from the pattern alone.
///
/// rawler's `interpolate_rb_at_green` (`imgop/sensor/bayer/ppg.rs:185-203`)
/// fills a green photosite's two missing channels from exactly
/// `(row, col+1)` and `(row+1, col)`, on the Bayer axiom that those two
/// carry the two DIFFERENT chroma colours. Every time a neighbour is green
/// instead, the write lands back on the green channel and that chroma
/// value is never written by any pass — `interpolate_rb_at_non_green`
/// (`ppg.rs:220-252`) is gated on `color_at != G` and never revisits a
/// green site.
///
/// The four 2×2 blocks contribute 2 + 1 + 1 failures each (top-left corner
/// both ways, top-right down, bottom-left right), so **16 chroma values
/// per 36-pixel tile are lost**, split 8 R / 8 B by the tile's R↔B duality.
/// Confirmed on the real X-S10 RAF before the fix: binning the 6252×4176
/// camera-native demosaic by `(row mod 6, col mod 6)` gave R = 0.0 at
/// 99.8 % of the pixels in 8 of the 36 phases and B = 0.0 in a different 8,
/// green in none — the residual 0.2 % being the 3-px ring that upstream's
/// CFA-correct border pass (`ppg.rs:74-110`) does reach.
///
/// Kept as a PATTERN assertion rather than a call into `PPGDemosaic`
/// deliberately: on a 6×6 CFA those passes read the buffer they are
/// concurrently writing through `Color2DPtr` (`pixarray.rs:500-529`), so
/// running them here would put a genuine data race in the suite to assert
/// on its output.
///
/// MUTATION THIS CATCHES: a future rawler that fixes X-Trans does not make
/// this red — it makes [`demosaic_over_cfa_geometry`] redundant, which is
/// a decision, not a regression. What it pins is the arithmetic behind the
/// 16, so nobody re-derives it from memory.
#[test]
fn the_bayer_chroma_axiom_fails_sixteen_times_per_x_trans_tile() {
    use rawler::cfa::{CFA, CFA_COLOR_G};
    let cfa = CFA::new(XTRANS_XS10);
    let lost: usize = (0..36)
        .map(|i| {
            let (r, c) = (i / 6, i % 6);
            if cfa.color_at(r, c) != CFA_COLOR_G {
                return 0;
            }
            usize::from(cfa.color_at(r, c + 1) == CFA_COLOR_G)
                + usize::from(cfa.color_at(r + 1, c) == CFA_COLOR_G)
        })
        .sum();
    assert_eq!(lost, 16, "the Bayer chroma axiom must fail 16 times per X-Trans tile");
}

/// The fix, on the case the defect destroyed: a flat colour must survive
/// EVERY CFA phase exactly. On the pre-fix path 16 of every 36 pixels came
/// out with a channel at 0.0 or half its value.
///
/// The ROI deliberately starts at neither the frame origin nor a tile
/// boundary — the X-S10's active area starts at `y = 5` (measured) — so a
/// demosaic that forgot to shift the pattern by the ROI origin writes the
/// measured photosite into the wrong channel and this goes red at the
/// first pixel.
///
/// MUTATION THIS CATCHES: drop `cfa.shift(roi.x(), roi.y())`; swap the R
/// and B tap sets; index the source plane from the frame origin instead of
/// the ROI's; mirror instead of fold at the border (the outer ring then
/// samples the wrong colour).
#[test]
fn the_cfa_geometry_demosaic_is_exact_on_flat_colour_at_every_phase() {
    use rawler::cfa::CFA;
    use rawler::imgop::{Dim2, Point, Rect};
    let cfa = CFA::new(XTRANS_XS10);
    let truth = [0.2f32, 0.5, 0.3];
    let (pw, ph) = (72usize, 72usize);
    let roi = Rect::new(Point::new(3, 5), Dim2::new(60, 60));
    let plane: Vec<f32> =
        (0..pw * ph).map(|i| truth[cfa.color_at(i / pw, i % pw)]).collect();
    let out = demosaic_over_cfa_geometry(&plane, Dim2::new(pw, ph), &cfa, roi);
    assert_eq!(out.len(), roi.width() * roi.height());
    let mut sums = [[0.0f64; 3]; 36];
    for (i, px) in out.iter().enumerate() {
        let (row, col) = (i / roi.width(), i % roi.width());
        for ch in 0..3 {
            assert!(
                (px[ch] - truth[ch]).abs() < 1e-5,
                "phase ({}, {}) channel {ch} came out {} instead of {}",
                row % 6,
                col % 6,
                px[ch],
                truth[ch]
            );
            sums[(row % 6) * 6 + col % 6][ch] += px[ch] as f64;
        }
    }
    // The whole-frame statement of the same thing, and the shape the
    // defect was originally measured in: 36 phase means per channel, which
    // must not spread at all on a flat field.
    let n = (out.len() / 36) as f64;
    for ch in 0..3 {
        let means: Vec<f64> = sums.iter().map(|s| s[ch] / n).collect();
        let spread = means.iter().cloned().fold(f64::MIN, f64::max)
            - means.iter().cloned().fold(f64::MAX, f64::min);
        assert!(spread < 1e-5, "channel {ch} spreads {spread} across the 36 CFA phases");
    }
}

/// …and on a LINEAR gradient, which is what makes the taps a plane fit
/// rather than a distance-weighted mean. Measured on this same synthetic
/// (60×60, interior): a mean's per-phase R/G ratio spreads by 7.2e-3 — a
/// 1.8 % chroma modulation at the tile period, i.e. visible fixed-pattern
/// chroma on a sky — where the plane fit spreads by 3.3e-16.
///
/// Interior only: outside `CfaTaps::radius` of the ROI edge the window
/// folds back into the frame by whole CFA repeats, which is colour-correct
/// but not linear.
#[test]
fn the_cfa_geometry_demosaic_is_exact_on_a_linear_gradient() {
    use rawler::cfa::CFA;
    use rawler::imgop::{Dim2, Point, Rect};
    let cfa = CFA::new(XTRANS_XS10);
    let (pw, ph) = (72usize, 72usize);
    let roi = Rect::new(Point::new(3, 5), Dim2::new(60, 60));
    let truth = |row: usize, col: usize| {
        let t = 0.1 + 0.6 * col as f32 / 71.0 + 0.2 * row as f32 / 71.0;
        [0.4 * t, t, 0.6 * t]
    };
    let plane: Vec<f32> = (0..pw * ph)
        .map(|i| truth(i / pw, i % pw)[cfa.color_at(i / pw, i % pw)])
        .collect();
    let out = demosaic_over_cfa_geometry(&plane, Dim2::new(pw, ph), &cfa, roi);
    let guard = cfa.width.max(cfa.height);
    for (i, px) in out.iter().enumerate() {
        let (row, col) = (i / roi.width(), i % roi.width());
        if row < guard || col < guard || row + guard >= roi.height() || col + guard >= roi.width()
        {
            continue;
        }
        let want = truth(roi.y() + row, roi.x() + col);
        for ch in 0..3 {
            assert!(
                (px[ch] - want[ch]).abs() < 2e-5,
                "({row}, {col}) channel {ch}: {} vs {}",
                px[ch],
                want[ch]
            );
        }
    }
}

/// Every tap set is a normalised, NON-NEGATIVE combination — the two
/// properties the doc comment's quality claims rest on. Sum = 1 is what
/// makes flat colour exact; non-negativity makes each estimate a convex
/// combination of real samples of that colour, so it cannot leave their
/// range and cannot ring. The second is a MEASURED property of this tile
/// at radius 2 (worst tap +0.056 over all 108 sets), not a theorem about
/// least squares — a different geometry may well need the guarantee
/// dropped, and this is where that would be noticed.
#[test]
fn every_cfa_tap_set_is_a_normalised_convex_combination() {
    use rawler::cfa::CFA;
    let cfa = CFA::new(XTRANS_XS10);
    let taps = cfa_taps(&cfa);
    assert_eq!(taps.per_phase.len(), 6 * 6 * 3);
    assert!(taps.radius >= 6, "the fallback window must span a whole repeat");
    for (i, set) in taps.per_phase.iter().enumerate() {
        let (phase, ch) = (i / 3, i % 3);
        assert!(!set.is_empty(), "phase {phase} channel {ch} has no sample at all");
        let sum: f32 = set.iter().map(|t| t.2).sum();
        assert!(
            (sum - 1.0).abs() < 1e-5,
            "phase {phase} channel {ch} weights sum to {sum}"
        );
        let worst = set.iter().map(|t| t.2).fold(f32::MAX, f32::min);
        assert!(worst >= 0.0, "phase {phase} channel {ch} has a negative tap {worst}");
    }
}

/// Out-of-frame taps fold by whole CFA repeats, so an offset keeps its
/// COLOUR. A mirror or a clamp puts a different colour under it and
/// reintroduces the channel error the whole path exists to remove.
#[test]
fn out_of_frame_taps_fold_by_whole_cfa_repeats() {
    let t = wrap_table(60, 6, 6);
    assert_eq!(t.len(), 60 + 12);
    for (i, &src) in t.iter().enumerate() {
        let logical = i as isize - 6;
        assert!(src < 60, "index {logical} folded to {src}, outside the frame");
        assert_eq!(
            src % 6,
            logical.rem_euclid(6) as usize,
            "index {logical} folded to {src} and changed colour"
        );
    }
    // A frame that is not a whole number of repeats folds by the largest
    // whole span inside it (54 of 57 rows here), never by the frame size.
    let t = wrap_table(57, 6, 6);
    for (i, &src) in t.iter().enumerate() {
        let logical = i as isize - 6;
        assert!(src < 57);
        assert_eq!(src % 6, logical.rem_euclid(6) as usize, "{logical} -> {src}");
    }
}

/// The dispatch is GEOMETRIC. Every Bayer spelling stays on rawler's own
/// develop (byte-identical: all eight non-Fuji zoo renders hashed the same
/// before and after this change), a 4-colour array is not ours to touch,
/// and only a non-2×2 RGB repeat takes the new path.
#[test]
fn only_a_non_two_by_two_rgb_cfa_takes_the_geometry_path() {
    use rawler::cfa::CFA;
    for bayer in ["RGGB", "BGGR", "GRBG", "GBRG"] {
        assert!(
            !cfa_needs_geometry_demosaic(&CFA::new(bayer)),
            "{bayer} is a 2×2 quincunx and must keep rawler's PPG"
        );
    }
    assert!(!cfa_needs_geometry_demosaic(&CFA::new("RGBE")), "4-colour is refused earlier");
    assert!(cfa_needs_geometry_demosaic(&CFA::new(XTRANS_XS10)));
}
