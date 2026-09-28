// One part of the sidecar's tests (src/xmp/tests.rs includes it): import bands, range masks, crs keys, the tint pin, pre-rename sidecars, hostile numbers, HSL and colour grade and the merge.

/// R25 P0-0.1: the bands `unparsable_crs_numbers` judges a document by ARE
/// the control registry's, not a second hand-written copy — so a new
/// attribute row arrives with its check already wired, and the families one
/// row cannot state are the only hand-written numbers left.
///
/// v0.31.1 removed the third residue. `Sharpness` needed its own band only
/// because the reader scaled it; now that the key is 1:1 with the recipe
/// row, the row's own 0..150 IS the document's band — the special case died
/// of the evidence, which is the shape a correct derivation should take.
#[test]
fn import_bands_are_the_registry_bands() {
    use crate::advisor::catalogue::RECIPE_CONTROLS;
    let mut checked = 0;
    for c in RECIPE_CONTROLS.iter() {
        let (Some(key), Some((lo, hi))) = (c.crs.attr(), c.range) else { continue };
        checked += 1;
        // `Sharpness` used to be excepted here, on a hand-written 0..100
        // sidecar band — v0.31.1 deleted the exception along with the
        // scale it stood for, so this key now derives like every other.
        // A full span outside each end (never a multiple of the bound: for
        // 2000..40000, `lo * 10` lands back INSIDE).
        let span = (hi - lo).max(1.0);
        assert!(crs_number_is_in_recipe_range(key, lo), "{key}: {lo} is the row's own floor");
        assert!(crs_number_is_in_recipe_range(key, hi), "{key}: {hi} is the row's own ceiling");
        assert!(!crs_number_is_in_recipe_range(key, hi + span), "{key}: above {hi} is out");
        assert!(!crs_number_is_in_recipe_range(key, lo - span), "{key}: below {lo} is out");
    }
    assert!(checked >= 15, "the registry stopped naming attribute rows: {checked}");
    // The three residues the registry cannot state, each derived from the
    // clamp that enforces it.
    for (key, inside, outside) in [
        ("SplitToningShadowHue", 359.0, 361.0),   // ColorGrade::clamp hue 0..360
        ("ColorGradeGlobalSat", 100.0, -1.0),     //                  sat 0..100
        ("ColorGradeBlending", 0.0, 101.0),       //             blending 0..100
        ("SplitToningBalance", -100.0, -101.0),   //              balance ±100
        ("ColorGradeShadowLum", 100.0, 101.0),    //                  lum ±100
        ("CropRight", 1.0, 1.5),                  // Crop::clamp 0..1
        ("HueAdjustmentRed", -100.0, 101.0),      // Hsl::clamp ±100
    ] {
        assert!(crs_number_is_in_recipe_range(key, inside), "{key}: {inside} must be legal");
        assert!(!crs_number_is_in_recipe_range(key, outside), "{key}: {outside} must not be");
    }
    // …and the derivation is what the DISCLOSURE reads: a Contrast2012
    // outside the `contrast` row's band is named, one inside is not.
    let doc = |v: &str| {
        format!(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                 xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
                 <rdf:Description rdf:about=\"\" \
                 xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
                 crs:Contrast2012=\"{v}\"/></rdf:RDF></x:xmpmeta>"
        )
    };
    assert!(unparsable_crs_numbers(&doc("150")).iter().any(|k| k == "Contrast2012"));
    assert!(unparsable_crs_numbers(&doc("50")).is_empty(), "an in-band value says nothing");
}

#[test]
fn renders_range_masks_as_intersected_components() {
    use crate::recipe::RangeMask;
    let r = EditRecipe {
        masks: vec![
            LocalAdjustment {
                mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.35, full_x: 0.5, full_y: 0.0 },
                range: Some(RangeMask::Luminance { lo_outer: 0.4, lo: 0.5, hi: 1.0, hi_outer: 1.0 }),
                name: "sky".into(),
                highlights: -40.0,
                ..Default::default()
            },
            LocalAdjustment {
                mask: MaskGeometry::Radial {
                    top: 0.3, left: 0.35, bottom: 0.7, right: 0.65,
                    feather: 0.5, roundness: 0.0, flipped: false, angle: 0.0,
                    midpoint: 50.0, mask_version: 2,
                },
                range: Some(RangeMask::Color { r: 0.9, g: 0.6, b: 0.2, amount: 0.5, px: 0.4, py: 0.7 }),
                name: "subject".into(),
                saturation: 20.0,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let xmp = recipe_to_xmp(&r);
    // Both range components present, encoded as intersections (the decoded
    // ACR algebra: BlendMode 1 + Inverted true + Value 0 = keep only where
    // the range matches).
    assert_eq!(xmp.matches(r#"crs:What="Mask/RangeMask""#).count(), 2);
    assert_eq!(
        xmp.matches(r#"crs:MaskBlendMode="1" crs:MaskInverted="true""#).count(), 2
    );
    // Luminance: attribute form, LumRange in ACR's 4-number trapezoid.
    assert!(xmp.contains(r#"crs:Type="2""#));
    assert!(xmp.contains(r#"crs:LumRange="0.400000 0.500000 1.000000 1.000000""#));
    // Colour: child-element form with one PointModels entry.
    assert!(xmp.contains(r#"crs:Type="1""#));
    assert!(xmp.contains(r#"crs:ColorAmount="0.500000""#));
    assert!(xmp.contains("<rdf:li>0.900000 0.600000 0.200000 0.400000 0.700000 0</rdf:li>"));
    // A mask WITHOUT a range emits no RangeMask component at all.
    let plain = EditRecipe {
        masks: vec![LocalAdjustment { name: "plain".into(), ..Default::default() }],
        ..Default::default()
    };
    assert!(!recipe_to_xmp(&plain).contains("RangeMask"));
}

#[test]
fn renders_expected_crs_keys() {
    let r = EditRecipe {
        exposure_ev: 0.32,
        contrast: 14.0,
        highlights: -12.0,
        temperature_k: Some(5600.0),
        tint: 3.0,
        sharpening: 45.0, // -> Sharpness 45, 1:1
        tone_curve: vec![
            CurvePoint { input: 0, output: 0 },
            CurvePoint { input: 255, output: 255 },
        ],
        rationale: "warm & contrasty <test> & \"q\"".into(),
        confidence: 0.82,
        ..Default::default()
    };
    let xmp = recipe_to_xmp(&r);
    assert!(xmp.contains(r#"crs:ProcessVersion="15.4""#));
    assert!(xmp.contains(r#"crs:Exposure2012="0.32""#));
    assert!(xmp.contains(r#"crs:Contrast2012="+14""#));
    assert!(xmp.contains(r#"crs:Highlights2012="-12""#));
    assert!(xmp.contains(r#"crs:WhiteBalance="Custom""#));
    assert!(xmp.contains(r#"crs:Temperature="5600""#));
    // 1:1 since v0.31.1 — a rendered 45 is written as 45. It used to be
    // written as 30, i.e. what the user saw was not what the sidecar said.
    assert!(xmp.contains(r#"crs:Sharpness="45""#));
    assert!(xmp.contains("<crs:ToneCurvePV2012>"));
    assert!(xmp.contains("<rdf:li>0, 0</rdf:li>"));
    // rationale is XML-escaped in the comment
    assert!(xmp.contains("&lt;test&gt;"));
}

#[test]
fn tint_only_edit_on_a_stamped_photo_pins_custom_at_as_shot() {
    // Stamped photo, tint-only: Custom AT the as-shot Kelvin — Lightroom
    // then applies the Tint instead of ignoring it under "As Shot".
    let r = EditRecipe { tint: 15.0, as_shot_k: Some(4820.0), ..Default::default() };
    let xmp = recipe_to_xmp(&r);
    assert!(xmp.contains(r#"crs:WhiteBalance="Custom""#), "{xmp}");
    assert!(xmp.contains(r#"crs:Temperature="4820""#), "{xmp}");
    assert!(xmp.contains(r#"crs:Tint="+15""#), "{xmp}");
    // Round trip of the PROJECTION: the import reads Custom back as an
    // absolute target == the stamp, which the anchored engine renders as
    // "no Kelvin shift".
    let back = xmp_to_recipe(&bare_document(&r, None));
    assert_eq!(back.temperature_k, Some(4820.0));
    assert_eq!(back.tint, 15.0);
    // The whole document (v1.3.1) restores the app's own state instead:
    // a tint-only edit over the stamp, no explicit Kelvin at all.
    let whole = xmp_to_recipe(&xmp);
    assert_eq!((whole.temperature_k, whole.as_shot_k, whole.tint), (None, Some(4820.0), 15.0));
    // A legacy recipe (no stamp) keeps the old honest fallback.
    let legacy = EditRecipe { tint: 15.0, ..Default::default() };
    let xmp = recipe_to_xmp(&legacy);
    assert!(xmp.contains(r#"crs:WhiteBalance="As Shot""#), "{xmp}");
    assert!(xmp.contains(r#"crs:Tint="+15""#), "{xmp}");
    // The engine-only stamp itself NEVER appears in a sidecar.
    assert!(!xmp.contains("as_shot"), "{xmp}");
}

/// Every sidecar already on a user's disk was stamped `x:xmptk="Autoshop"`
/// or `"Autoshop 2"`, and that token is a RENDERING decision, not a label:
/// era-2 Temperature is absolute, era-1 is relative to the 5500 K anchor.
/// Reading a pre-rename era-2 document as era-1 would pin 5500 onto an
/// absolute value and shift the white balance of every develop the user
/// ever saved.
///
/// MUTATION: drop `XMPTK_ERA2_PRE_RENAME` from `is_autoshade_era2` and the
/// pre-rename era-2 document below comes back with `as_shot_k` pinned.
#[test]
fn a_pre_rename_sidecar_keeps_its_era_and_its_provenance() {
    let doc = |tk: &str, temp: &str| {
        format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="{tk}">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    crs:WhiteBalance="Custom"
    crs:Temperature="{temp}"
    crs:HasSettings="True">
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#
        )
    };

    // Era 2 under BOTH spellings: absolute Kelvin, no anchor pin.
    for tk in ["AutoShade 2", "Autoshop 2"] {
        let r = xmp_to_recipe(&doc(tk, "5000"));
        assert!(is_autoshade_era2(&doc(tk, "5000")), "{tk} lost its era-2 marker");
        assert!(is_autoshade_sidecar(&doc(tk, "5000")), "{tk} lost its provenance");
        assert_eq!(r.temperature_k, Some(5000.0), "{tk}");
        assert_eq!(r.as_shot_k, None, "{tk}: era-2 Kelvin is absolute — no pin");
    }
    // Era 1 under BOTH spellings: ours, and pinned to the legacy anchor.
    for tk in ["AutoShade", "Autoshop"] {
        let r = xmp_to_recipe(&doc(tk, "5000"));
        assert!(!is_autoshade_era2(&doc(tk, "5000")), "{tk} claimed era 2");
        assert!(is_autoshade_sidecar(&doc(tk, "5000")), "{tk} lost its provenance");
        assert_eq!(r.as_shot_k, Some(5500.0), "{tk}: era-1 pins the legacy anchor");
    }
    // A foreign toolkit is still foreign, and a prefix of ours is not ours.
    assert!(!is_autoshade_sidecar(&doc("Adobe XMP Core 7.0-c000", "5000")));
    assert!(!is_autoshade_sidecar(&doc("AutoShadester", "5000")));
    assert!(!is_autoshade_sidecar(&doc("Autoshopping", "5000")));
}

/// A merge rewrites the owned white-balance attributes in ABSOLUTE
/// semantics, so it must leave an era-2 marker behind whichever era-1
/// spelling it found — under the CURRENT name, since that is what this
/// build writes.
///
/// MUTATION: upgrade only the current era-1 spelling and a pre-rename
/// document keeps its era-1 marker, so the next import pins 5500 onto the
/// absolute values this merge just wrote.
#[test]
fn a_pre_rename_era_marker_upgrades_when_the_merge_makes_it_absolute() {
    for tk in ["AutoShade", "Autoshop"] {
        let doc = format!(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="{tk}">"#);
        let up = upgrade_era_marker(doc);
        assert_eq!(
            up, r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="AutoShade 2">"#,
            "{tk} did not reach the current era-2 marker"
        );
        assert!(is_autoshade_era2(&up), "{tk} upgraded to something unreadable");
    }
    // Already era 2 under either spelling: untouched, never double-upgraded.
    for tk in ["AutoShade 2", "Autoshop 2"] {
        let doc = format!(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="{tk}">"#);
        assert_eq!(upgrade_era_marker(doc.clone()), doc, "{tk} was double-upgraded");
    }
}

/// The rationale comment is an on-disk token too: a pre-rename sidecar
/// carries `<!-- Generated by Autoshop. AI rationale: … -->`, and a merge
/// that cannot find it leaves the OLD reasoning attached to a NEW recipe.
///
/// MUTATION: look for the current mark only and the pre-rename document
/// below keeps its stale rationale.
#[test]
fn a_pre_rename_rationale_comment_is_still_refreshed() {
    let fresh = EditRecipe { confidence: 0.5, rationale: "the new reason".into(), ..Default::default() };
    for mark in ["AutoShade", "Autoshop"] {
        let doc = format!("<x:xmpmeta><!-- Generated by {mark}. AI rationale: the stale reason (confidence 0.90) -->\n</x:xmpmeta>");
        let out = refresh_rationale_comment(doc, &fresh);
        assert!(out.contains("the new reason"), "{mark}: the stale rationale survived");
        assert!(!out.contains("the stale reason"), "{mark}: both rationales are present");
        assert!(
            out.contains("<!-- Generated by AutoShade. AI rationale: "),
            "{mark}: the refreshed comment must carry the current name"
        );
    }
}

#[test]
fn legacy_autoshade_sidecar_kelvin_stays_relative_via_the_anchor_pin() {
    // A sidecar WE wrote before the absolute-Kelvin engine: its
    // Temperature was tuned against the 5500 K anchor. The import pins
    // the anchor there, so every stamp-if-None caller leaves it alone
    // and the develop renders exactly as tuned.
    let old = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="AutoShade">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    crs:WhiteBalance="Custom"
    crs:Temperature="5000"
    crs:HasSettings="True">
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;
    let r = xmp_to_recipe(old);
    assert_eq!(r.temperature_k, Some(5000.0));
    assert_eq!(r.as_shot_k, Some(5500.0), "old-era Kelvin pins the legacy anchor");
    assert_eq!(r.as_shot_tint, None, "the pin claims no camera as-shot");
    // Era-2 documents (what this build writes) stay unpinned — their
    // Temperature is absolute and the caller stamps the real camera K.
    let new =
        recipe_to_xmp(&EditRecipe { temperature_k: Some(5000.0), ..Default::default() });
    assert!(new.contains(r#"x:xmptk="AutoShade 2""#), "{new}");
    let r2 = xmp_to_recipe(&new);
    assert_eq!(r2.temperature_k, Some(5000.0));
    assert_eq!(r2.as_shot_k, None, "era-2 Kelvin is absolute — no pin");
    // Foreign (Lightroom) sidecars are never pinned either.
    let lr = old.replace("AutoShade", "Adobe XMP Core 7.0-c000");
    assert_eq!(xmp_to_recipe(&lr).as_shot_k, None, "foreign Kelvin is absolute");
    // A6 disclosure scanner: corrupt numbers are NAMED; parsable and
    // string-typed keys never flag; our own writer round-trips clean.
    let corrupt = old
        .replace(r#"crs:Temperature="5000""#, r#"crs:Temperature="fivethousand""#)
        .replace(
            r#"crs:HasSettings="True""#,
            "crs:Contrast2012=\"NaNny\"\n    crs:Exposure2012=\"+0.65\"\n    crs:HasSettings=\"True\"",
        );
    let bad = unparsable_crs_numbers(&corrupt);
    assert!(bad.contains(&"Temperature".to_string()), "{bad:?}");
    assert!(bad.contains(&"Contrast2012".to_string()), "{bad:?}");
    assert!(!bad.contains(&"Exposure2012".to_string()), "{bad:?}");
    assert!(!bad.iter().any(|k| k == "WhiteBalance" || k == "HasSettings"), "{bad:?}");
    assert_eq!(xmp_to_recipe(&corrupt).contrast, 0.0, "the silent neutral being disclosed");
    let clean = recipe_to_xmp(&EditRecipe {
        exposure_ev: 0.4,
        temperature_k: Some(5600.0),
        ..Default::default()
    });
    assert!(unparsable_crs_numbers(&clean).is_empty());
    // A MERGE into an old AutoShade document rewrites the WB attributes in
    // absolute semantics — the era marker must upgrade with them.
    let merged = merged_doc(
        old,
        &EditRecipe { temperature_k: Some(6200.0), ..Default::default() },
    )
    .expect("mergeable");
    assert!(merged.contains(r#"x:xmptk="AutoShade 2""#), "{merged}");
    assert!(!merged.contains(r#"x:xmptk="AutoShade""#) || merged.contains("AutoShade 2"));
    assert_eq!(xmp_to_recipe(&merged).as_shot_k, None, "upgraded doc is not pinned");
}

#[test]
fn non_finite_numbers_import_neutral_and_are_disclosed() {
    // Rust's f32 parser accepts "NaN" and "inf"; no real sidecar writer
    // emits them. They must import as neutral AND be named by the
    // disclosure scanner — the old exact-parse mirror read them as
    // "fine", so the silent neutral was never disclosed.
    let lr = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core 7.0-c000">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    crs:WhiteBalance="Custom"
    crs:Temperature="NaN"
    crs:Contrast2012="inf"
    crs:Exposure2012="+0.65"
    crs:HasSettings="True">
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;
    let lr_scope = Scope::new(lr);
    assert_eq!(lr_scope.crs_f32("Temperature"), None, "NaN is not a Kelvin");
    assert_eq!(lr_scope.crs_f32("Contrast2012"), None, "inf is not a slider");
    let r = xmp_to_recipe(lr);
    assert_eq!(r.temperature_k, None);
    assert_eq!(r.contrast, 0.0);
    assert_eq!(r.exposure_ev, 0.65, "finite neighbours still import");
    let bad = unparsable_crs_numbers(lr);
    assert!(bad.contains(&"Temperature".to_string()), "{bad:?}");
    assert!(bad.contains(&"Contrast2012".to_string()), "{bad:?}");
    assert!(!bad.contains(&"Exposure2012".to_string()), "{bad:?}");
}

/// 16-lane scan L05: "999, -5" used to saturate to (255, 0) — a one-point
/// master curve that renders nearly black, imported silently and
/// PERSISTED by the next save. Out-of-domain now takes the same
/// reject-and-disclose path as a malformed point.
#[test]
fn out_of_domain_curve_points_drop_the_curve_and_are_disclosed() {
    let lr = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core 7.0-c000">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    crs:Exposure2012="+0.30"
    crs:HasSettings="True">
   <crs:ToneCurvePV2012>
    <rdf:Seq>
     <rdf:li>999, -5</rdf:li>
    </rdf:Seq>
   </crs:ToneCurvePV2012>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;
    let r = xmp_to_recipe(lr);
    assert!(r.tone_curve.is_empty(), "the out-of-domain curve must not import");
    assert_eq!(r.exposure_ev, 0.30, "finite neighbours still import");
    let bad = unparsable_crs_numbers(lr);
    assert!(bad.contains(&"ToneCurvePV2012".to_string()), "{bad:?}");
    // In-domain float spellings keep rounding like before (a non-identity
    // pair — the 0,0→255,255 identity deliberately collapses to empty).
    assert_eq!(
        parse_curve_checked("<crs:T><rdf:Seq><rdf:li>0, 10</rdf:li><rdf:li>254.6, 255</rdf:li></rdf:Seq></crs:T>", "T"),
        Ok(vec![
            CurvePoint { input: 0, output: 10 },
            CurvePoint { input: 255, output: 255 }
        ])
    );
}

/// 16-lane scan L05: a wheel whose HUE is unreadable must not keep its
/// paired saturation — the zero fallback made "bogus" hue 0 (= RED) and
/// a valid Saturation of 50 imported as a strong red grade while the
/// disclosure claimed neutral restoration.
#[test]
fn an_unreadable_wheel_hue_zeroes_its_paired_saturation() {
    let lr = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core 7.0-c000">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    crs:SplitToningShadowHue="bogus"
    crs:SplitToningShadowSaturation="50"
    crs:SplitToningHighlightHue="45"
    crs:SplitToningHighlightSaturation="20"
    crs:HasSettings="True">
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;
    let r = xmp_to_recipe(lr);
    assert_eq!(
        r.color_grade.shadow_sat, 0.0,
        "an unreadable shadow hue must take its saturation with it"
    );
    assert_eq!(r.color_grade.highlight_hue, 45.0, "the healthy wheel is untouched");
    assert_eq!(r.color_grade.highlight_sat, 20.0);
    assert!(
        unparsable_crs_numbers(lr).contains(&"SplitToningShadowHue".to_string()),
        "and the unreadable hue is named"
    );
}

#[test]
fn renders_hsl_bands_only_when_set() {
    let r = EditRecipe {
        hsl: crate::recipe::Hsl {
            hue: [0.0, 15.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], // orange +15
            saturation: [0.0, 0.0, 0.0, -40.0, 0.0, 0.0, 0.0, 0.0], // green -40
            ..Default::default()
        },
        ..Default::default()
    };
    let xmp = recipe_to_xmp(&r);
    assert!(xmp.contains(r#"crs:HueAdjustmentOrange="+15""#));
    assert!(xmp.contains(r#"crs:SaturationAdjustmentGreen="-40""#));
    assert!(xmp.contains(r#"crs:LuminanceAdjustmentRed="0""#)); // full 24-key block
    // A neutral recipe emits NO HSL keys (minimal, v1-compatible sidecar).
    assert!(!recipe_to_xmp(&EditRecipe::default()).contains("HueAdjustment"));
}

#[test]
fn renders_color_grade_with_verified_split_toning_mapping() {
    let r = EditRecipe {
        color_grade: crate::recipe::ColorGrade {
            shadow_hue: 220.0, shadow_sat: 30.0,
            highlight_hue: 45.0, highlight_sat: 20.0,
            midtone_lum: -10.0, balance: 15.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let xmp = recipe_to_xmp(&r);
    // shadow/highlight hue+sat round-trip via the legacy SplitToning* keys
    assert!(xmp.contains(r#"crs:SplitToningShadowHue="220""#));
    assert!(xmp.contains(r#"crs:SplitToningShadowSaturation="30""#));
    assert!(xmp.contains(r#"crs:SplitToningHighlightHue="45""#));
    assert!(xmp.contains(r#"crs:SplitToningBalance="+15""#));
    // lum / midtone / global / blending via ColorGrade*
    assert!(xmp.contains(r#"crs:ColorGradeMidtoneLum="-10""#));
    assert!(xmp.contains(r#"crs:ColorGradeBlending="50""#)); // ACR default
    // A neutral recipe emits NO grading keys at all.
    let neutral = recipe_to_xmp(&EditRecipe::default());
    assert!(!neutral.contains("ColorGrade") && !neutral.contains("SplitToning"));
}

#[test]
fn renders_per_channel_rgb_curves() {
    let r = EditRecipe {
        red_curve: vec![CurvePoint { input: 0, output: 10 }, CurvePoint { input: 255, output: 250 }],
        blue_curve: vec![
            CurvePoint { input: 0, output: 0 },
            CurvePoint { input: 128, output: 110 },
            CurvePoint { input: 255, output: 255 },
        ],
        ..Default::default()
    };
    let xmp = recipe_to_xmp(&r);
    assert!(xmp.contains("<crs:ToneCurvePV2012Red>"));
    assert!(xmp.contains("<rdf:li>0, 10</rdf:li>"));
    assert!(xmp.contains("<crs:ToneCurvePV2012Blue>"));
    assert!(xmp.contains("<rdf:li>128, 110</rdf:li>"));
    // The empty green channel emits no element.
    assert!(!xmp.contains("ToneCurvePV2012Green"));
    // A neutral recipe emits no per-channel curves at all.
    assert!(!recipe_to_xmp(&EditRecipe::default()).contains("ToneCurvePV2012Red"));
}

// ── merge (merge_recipe_into_xmp) ────────────────────────────────────────

#[test]
fn merge_preserves_lightroom_only_properties() {
    let lr = "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Adobe XMP Core 7.0-c000\">\n\
 <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
  <rdf:Description rdf:about=\"\"\n\
    xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n\
    xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n\
    crs:Version=\"15.5.1\"\n\
    crs:ProcessVersion='15.4'\n\
    crs:PointColor=\"0\"\n\
    crs:CameraProfile=\"Adobe Color\"\n\
    crs:LensProfileEnable=\"1\"\n\
    crs:LensProfileName=\"Sony FE 24-70 > special\"\n\
    crs:Exposure2012=\"+1.00\"\n\
    crs:HasSettings=\"True\">\n\
   <crs:ToneCurvePV2012>\n\
    <rdf:Seq>\n\
     <rdf:li>0, 0</rdf:li>\n\
     <rdf:li>255, 255</rdf:li>\n\
    </rdf:Seq>\n\
   </crs:ToneCurvePV2012>\n\
   <crs:Look>\n\
    <rdf:Description crs:Name=\"Adobe Color\" crs:Amount=\"1\"/>\n\
   </crs:Look>\n\
  </rdf:Description>\n\
 </rdf:RDF>\n\
</x:xmpmeta>\n";
    let r = EditRecipe {
        exposure_ev: 0.25,
        contrast: 12.0,
        tone_curve: vec![
            CurvePoint { input: 0, output: 10 },
            CurvePoint { input: 255, output: 250 },
        ],
        ..Default::default()
    };
    let merged = merged_doc(lr, &r).expect("a plain LR sidecar is mergeable");
    // Everything AutoShade does not model survives. (The sample used to be
    // `crs:Texture`; R25 B2 models it, so it is no longer an example of an
    // unmodelled global — it is an example of an owned one, which
    // `a_cleared_texture_disappears_from_a_merged_document` covers.)
    assert!(merged.contains("crs:PointColor=\"0\""), "an unmodelled global survives");
    assert!(merged.contains("crs:CameraProfile=\"Adobe Color\""), "camera profile survives");
    assert!(
        merged.contains("crs:LensProfileName=\"Sony FE 24-70 > special\""),
        "LR lens profile survives — even with '>' inside the value"
    );
    assert!(merged.contains("<crs:Look>"), "LR-only child elements survive");
    assert!(merged.contains("xmlns:dc="), "foreign namespaces survive");
    assert!(merged.starts_with("<?xpacket"), "the xpacket wrapper survives");
    // Ours REPLACE, never duplicate — including the single-quoted form
    // (legal XML; leaving it would duplicate the attribute).
    assert_eq!(merged.matches("crs:Exposure2012=").count(), 1);
    assert_eq!(merged.matches("crs:ProcessVersion=").count(), 1);
    assert!(merged.contains("crs:ProcessVersion=\"15.4\""), "replaced in OUR form");
    assert!(merged.contains("crs:Exposure2012=\"0.25\""));
    assert_eq!(merged.matches("<crs:ToneCurvePV2012>").count(), 1);
    assert!(merged.contains("<rdf:li>0, 10</rdf:li>"), "OUR curve, not Lightroom's");
    // The reader sees OUR values in the merged document.
    let back = xmp_to_recipe(&merged);
    assert_eq!((back.exposure_ev, back.contrast), (0.25, 12.0));
    // A second merge over the merged document stays single AND a cleared
    // curve REMOVES the block (a stale slider must not linger).
    let r2 = EditRecipe { exposure_ev: -0.5, ..Default::default() };
    let merged2 = merged_doc(&merged, &r2).expect("re-mergeable");
    assert_eq!(merged2.matches("crs:Exposure2012=").count(), 1);
    assert!(merged2.contains("crs:Exposure2012=\"-0.50\""));
    assert!(merged2.contains("crs:PointColor=\"0\""), "still there after a second merge");
    assert_eq!(merged2.matches("<crs:ToneCurvePV2012>").count(), 0, "cleared curve gone");
    assert!(merged2.contains("ToneCurveName2012=\"Linear\""));
}

#[test]
fn merge_strips_owned_element_form_properties() {
    // Lightroom serialises the SAME settings as property elements in
    // plenty of real sidecars (crs_str accepts that form). The merge
    // must strip the owned element too, or the document answers one
    // slider with two conflicting values — while unowned elements
    // (PointColor; it was Texture until R25 B2 modelled that one)
    // survive untouched.
    let lr = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Adobe XMP Core 7.0-c000\">\n\
 <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
  <rdf:Description rdf:about=\"\"\n\
    xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n\
    crs:HasSettings=\"True\">\n\
   <crs:Exposure2012>+1.00</crs:Exposure2012>\n\
   <crs:Contrast2012>+22</crs:Contrast2012>\n\
   <crs:PointColor>0</crs:PointColor>\n\
  </rdf:Description>\n\
 </rdf:RDF>\n\
</x:xmpmeta>\n";
    let r = EditRecipe { exposure_ev: 0.25, ..Default::default() };
    let merged = merged_doc(lr, &r).expect("mergeable");
    assert!(!merged.contains("<crs:Exposure2012>"), "owned element stripped: {merged}");
    assert!(!merged.contains("<crs:Contrast2012>"), "owned element stripped");
    assert_eq!(merged.matches("crs:Exposure2012").count(), 1, "ours only: {merged}");
    assert!(merged.contains("crs:Exposure2012=\"0.25\""));
    assert!(
        merged.contains("<crs:PointColor>0</crs:PointColor>"),
        "unowned element survives: {merged}"
    );
    let back = xmp_to_recipe(&merged);
    assert_eq!(back.exposure_ev, 0.25);
    assert_eq!(back.contrast, 0.0, "the old element value must not shadow the cleared slider");
}

#[test]
fn merge_strips_only_top_level_owned_elements() {
    // The strip is a property of THIS Description. Adobe writes a creative
    // profile's baked parameters as owned-LOOKING children of a nested
    // rdf:Description inside <crs:Look>, and a flat scan reached in and
    // gutted them — destroying the very Look this merge exists to
    // preserve. Name matching also catches the attribute-carrying
    // spelling, which the `<crs:Name>` literal missed (leaving exactly the
    // duplicate the element strip exists to prevent).
    let lr = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
 <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
  <rdf:Description rdf:about=\"\"\n\
    xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n\
    crs:HasSettings=\"True\">\n\
   <crs:Exposure2012 xml:lang=\"x-default\">+1.00</crs:Exposure2012>\n\
   <crs:Look>\n\
    <rdf:Description crs:Name=\"Adobe Landscape\" crs:Amount=\"1\">\n\
     <crs:Parameters>\n\
      <rdf:Description crs:Version=\"15.4\">\n\
       <crs:Exposure2012>+0.35</crs:Exposure2012>\n\
       <crs:ToneCurvePV2012>\n\
        <rdf:Seq><rdf:li>0, 0</rdf:li></rdf:Seq>\n\
       </crs:ToneCurvePV2012>\n\
      </rdf:Description>\n\
     </crs:Parameters>\n\
    </rdf:Description>\n\
   </crs:Look>\n\
  </rdf:Description>\n\
 </rdf:RDF>\n\
</x:xmpmeta>\n";
    let r = EditRecipe { exposure_ev: 0.25, ..Default::default() };
    let merged = merged_doc(lr, &r).expect("mergeable");
    // The Look keeps BOTH of its own baked parameters.
    assert!(
        merged.contains("<crs:Exposure2012>+0.35</crs:Exposure2012>"),
        "the Look's own parameter must survive: {merged}"
    );
    assert!(merged.contains("<rdf:li>0, 0</rdf:li>"), "the Look's own curve must survive");
    assert!(merged.contains("crs:Name=\"Adobe Landscape\""), "and the Look itself");
    // ...while OUR top-level property is stripped in the attribute-carrying
    // spelling too, leaving exactly one answer for the slider.
    assert!(!merged.contains("xml:lang"), "top-level owned element stripped: {merged}");
    assert!(merged.contains("crs:Exposure2012=\"0.25\""), "ours is the attribute");
    assert_eq!(
        merged.matches("crs:Exposure2012").count(),
        3,
        "ours + the Look's open/close, no shadow copy: {merged}"
    );
    assert_eq!(xmp_to_recipe(&merged).exposure_ev, 0.25);
}

#[test]
fn merge_survives_a_cdata_section() {
    // LEGAL XML must never fall back to a full regenerate: that path
    // replaces the user's whole sidecar with our own document and takes
    // every foreign property with it — the data loss the merge exists to
    // prevent. A CDATA section is not a tag; a scanner that counts it as
    // one leaves `depth` unbalanced and bails.
    let lr = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
 <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
  <rdf:Description rdf:about=\"\"\n\
    xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n\
    xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n\
    crs:HasSettings=\"True\">\n\
   <crs:Exposure2012>+1.00</crs:Exposure2012>\n\
   <dc:description><![CDATA[client <proof> notes]]></dc:description>\n\
   <crs:PointColor>0</crs:PointColor>\n\
  </rdf:Description>\n\
 </rdf:RDF>\n\
</x:xmpmeta>\n";
    let r = EditRecipe { exposure_ev: 0.25, ..Default::default() };
    let merged = merged_doc(lr, &r).expect("a CDATA section must stay mergeable");
    assert!(
        merged.contains("<![CDATA[client <proof> notes]]>"),
        "the foreign CDATA property must survive verbatim: {merged}"
    );
    assert!(merged.contains("<crs:PointColor>0</crs:PointColor>"), "unowned element survives");
    assert!(!merged.contains("<crs:Exposure2012>"), "ours is still stripped: {merged}");
    assert!(merged.contains("crs:Exposure2012=\"0.25\""));
}

#[test]
fn merge_replaces_masks_without_shredding_nested_descriptions() {
    // Lightroom nests rdf:Description elements INSIDE mask corrections —
    // the close-tag search must depth-count (the batch-3 lesson), and the
    // mask block is replaced wholesale while everything AFTER it lives.
    let lr = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
 <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
  <rdf:Description rdf:about=\"\"\n\
    xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n\
    crs:PointColor=\"0\"\n\
    crs:HasSettings=\"True\">\n\
   <crs:MaskGroupBasedCorrections>\n\
    <rdf:Seq>\n\
     <rdf:li>\n\
      <rdf:Description crs:What=\"Correction\" crs:LocalExposure2012=\"0.1\">\n\
       <crs:CorrectionMasks>\n\
        <rdf:Seq>\n\
         <rdf:li>\n\
          <rdf:Description crs:What=\"Mask/Gradient\" crs:ZeroX=\"0.5\" crs:ZeroY=\"0.4\" crs:FullX=\"0.5\" crs:FullY=\"0.0\"/>\n\
         </rdf:li>\n\
        </rdf:Seq>\n\
       </crs:CorrectionMasks>\n\
      </rdf:Description>\n\
     </rdf:li>\n\
    </rdf:Seq>\n\
   </crs:MaskGroupBasedCorrections>\n\
   <crs:Look>\n\
    <rdf:Description crs:Name=\"Adobe Landscape\"/>\n\
   </crs:Look>\n\
  </rdf:Description>\n\
 </rdf:RDF>\n\
</x:xmpmeta>\n";
    let r = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Radial {
                top: 0.2,
                left: 0.2,
                bottom: 0.8,
                right: 0.8,
                feather: 0.5,
                roundness: 0.0,
                flipped: false,
                angle: 0.0,
                midpoint: 50.0,
                mask_version: 2,
            },
            exposure_ev: 1.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let merged = merged_doc(lr, &r).expect("mergeable");
    assert_eq!(
        merged.matches("<crs:MaskGroupBasedCorrections>").count(),
        1,
        "one mask block — OURS"
    );
    assert!(merged.contains("Mask/CircularGradient"), "our radial mask is in");
    // The fully supported old correction is replaceable; its old local
    // exposure value must not survive beside the new radial correction.
    assert!(
        !merged.contains("crs:LocalExposure2012=\"0.1\""),
        "LR's fully supported old mask block is replaced"
    );
    assert!(!merged.contains("crs:ZeroX=\"0.5\""), "…including its nested gradient");
    assert!(
        merged.contains("crs:Name=\"Adobe Landscape\""),
        "the element AFTER the mask block survives — nesting was not shredded"
    );
    assert!(merged.contains("crs:PointColor=\"0\""), "unowned attribute survives");
    // The whole document still ends properly (splice did not eat the tail).
    assert!(merged.trim_end().ends_with("</x:xmpmeta>"));
}

// ── reader (xmp_to_recipe) ───────────────────────────────────────────────
