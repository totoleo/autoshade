// One part of the sidecar's tests (src/xmp/tests.rs includes it): Lightroom brush groups and the real sidecars read at run time.

#[test]
fn a_lightroom_brush_group_imports_beside_the_shapes_it_used_to_take_down() {
    let doc = lr_doc(&lr_correction(
        "Mask 7",
        "",
        &format!("{}{}", lr_gradient("0"), lr_brush_group("false", "")),
    ));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "the correction must import: {:?}", r.masks);
    let m = &r.masks[0];
    // The parametric shape is still the BASE — a brush does not displace a
    // gradient that was there first (`base_geometry_at`).
    assert!(matches!(m.mask, MaskGeometry::Linear { .. }), "{:?}", m.mask);
    assert_eq!(m.components.len(), 1, "the brush group rides as a component");
    assert_eq!(m.components[0].mode, MaskCombine::Add, "MaskBlendMode=0 is a union");
    let MaskGeometry::Brush { name, blend_mode, value, inverted, strokes } =
        &m.components[0].geometry
    else {
        panic!("expected a brush group, got {:?}", m.components[0].geometry);
    };
    assert_eq!((name.as_str(), *blend_mode, *value, *inverted), ("Brush 1", 0, 1.0, false));
    assert_eq!(strokes.len(), 2, "both Mask/Paint children arrive");
    assert_eq!(strokes[0].value, 0.439815);
    assert_eq!(strokes[0].radius, 0.582157);
    assert_eq!(strokes[0].flow, 1.0);
    assert_eq!(strokes[0].center_weight, 0.0);
    assert_eq!(strokes[0].sync_id, "FA7459A9F5626F4881D7B730C3093F95");
    // The dab stream, token for token, in document order.
    assert_eq!(
        strokes[0].dabs,
        "r 0.581835\nd 0.000684 0.940004\nr 0.581172\nd 0.113862 0.987261\n\
             r 0.580873\nd 0.229292 1.011389\nr 0.581205\nd 0.112441 1.007149"
    );
    assert_eq!(strokes[1].dabs, "f 1\nh 1\nd 0.500000 0.500000");
    // And the import SAYS so — imported whole, drawn from our model.
    let losses = import_losses(&doc);
    assert!(
        losses.iter().any(|l| l.reason == MaskImportReason::BrushRendered),
        "a brush drawn from our own model must be disclosed: {losses:?}"
    );
    assert!(
        !losses.iter().any(|l| l.reason.is_drop()),
        "nothing about this correction was dropped: {losses:?}"
    );
}

/// A correction whose ONLY component is a brush group imports too — its
/// first Aggregate becomes the base geometry (F2 §7.3). Nine of the
/// eighteen corrections this batch rescues have exactly that shape.
///
/// MUTATION-LINED: deleting `parse_one_correction`'s brush fallback (the
/// `None => { … brushes.remove(0) … }` arm) makes it return `None`, the
/// correction lands on `OutOfModel`, and this goes to 0 masks.
#[test]
fn a_brush_only_correction_takes_its_first_group_as_the_base() {
    let doc = lr_doc(&lr_correction("Mask 1", "", &lr_brush_group("false", "")));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "brush-only corrections import now: {:?}", r.masks);
    assert!(matches!(r.masks[0].mask, MaskGeometry::Brush { .. }));
    assert!(r.masks[0].components.is_empty(), "one group, no extras");
    // Inert, NOT inverted: the group's own inversion bit lives inside the
    // geometry, and lifting it here as well would turn a zero-coverage
    // mask into a whole-frame adjustment.
    assert!(!r.masks[0].inverted);
}

/// The write-back is faithful to the measured `Mask/Aggregate` shape:
/// import → write puts every attribute and every dab token back, and a
/// second import is a FIXED POINT. (The `crs:MaskSyncID`s are the writer's
/// own by design — it mints them for every component it emits — so the
/// comparison that has to be exact is the recipe, not the ID text.)
///
/// MUTATION-LINED: dropping `extra_lis` from the emitted `<rdf:Seq>` makes
/// the group vanish from the sidecar and the fixed-point half fails.
#[test]
fn a_brush_group_round_trips_back_into_the_sidecar() {
    // BOTH slots the group can occupy, because the writer reaches them
    // through different code: as the correction's BASE (a brush-only
    // correction) and as an extra COMPONENT beside a parametric shape
    // (`P12` Mask 7's own shape). A round-trip test that only ever
    // saw the base would stay green while the component arm dropped the
    // group on the floor.
    for components in [
        lr_brush_group("false", ""),
        format!("{}{}", lr_gradient("0"), lr_brush_group("false", "")),
    ] {
        brush_round_trip_case(&components);
    }
}

fn brush_round_trip_case(components: &str) {
    let doc = lr_doc(&lr_correction("Mask 7", "", components));
    let once = xmp_to_recipe(&doc);
    let written = recipe_to_xmp(&once);
    // 1. The dab tokens ride out verbatim, in order.
    for token in [
        "<rdf:li>r 0.581835</rdf:li>",
        "<rdf:li>d 0.000684 0.940004</rdf:li>",
        "<rdf:li>d 0.112441 1.007149</rdf:li>",
        "<rdf:li>f 1</rdf:li>",
        "<rdf:li>h 1</rdf:li>",
    ] {
        assert!(written.contains(token), "missing {token} in:\n{written}");
    }
    // 2. The components' own attributes, in Lightroom's own spelling.
    for attr in [
        r#"crs:What="Mask/Aggregate""#,
        r#"crs:MaskName="Brush 1""#,
        r#"crs:What="Mask/Paint""#,
        r#"crs:MaskValue="0.439815""#,
        r#"crs:Radius="0.582157""#,
        r#"crs:Flow="1""#,
        r#"crs:CenterWeight="0""#,
    ] {
        assert!(written.contains(attr), "missing {attr} in:\n{written}");
    }
    // 3. FIXED POINT, at the level that has to be one — the DOCUMENT.
    // Reading our own sidecar back and writing it again is byte-identical,
    // which is the assertion that fails if any value is reformatted on the
    // way out: an `f32` printed through a rounding formatter, a token
    // re-spaced, a stroke re-ordered, an attribute dropped.
    let twice = xmp_to_recipe(&written);
    assert_eq!(written, recipe_to_xmp(&twice), "the sidecar is not a fixed point");
    assert_eq!(once.masks.len(), twice.masks.len());
    let (a, b) = (&once.masks[0], &twice.masks[0]);
    // The RECIPE is a fixed point too, with exactly one NAMED exception:
    // `crs:MaskSyncID`. The writer mints its own for every component it
    // emits (`guid`), so a group that came in with Lightroom's IDs goes out
    // with ours and comes back carrying those. That is the writer's
    // standing rule rather than anything about brushes — and it is an
    // ACCEPTED COST, stated here so it is a decision and not a surprise:
    // the ID Lightroom used for a stroke survives one save and no more.
    // Everything that describes the STROKE survives every save.
    let strip = |g: &MaskGeometry| match g {
        MaskGeometry::Brush { name, blend_mode, value, inverted, strokes } => {
            let bare: Vec<_> = strokes
                .iter()
                .map(|s| BrushStroke { sync_id: String::new(), ..s.clone() })
                .collect();
            MaskGeometry::Brush {
                name: name.clone(),
                blend_mode: *blend_mode,
                value: *value,
                inverted: *inverted,
                strokes: bare,
            }
        }
        other => other.clone(),
    };
    assert_eq!(strip(&a.mask), strip(&b.mask), "the brush geometry is not a fixed point");
    assert_eq!(a.components.len(), b.components.len());
    for (ca, cb) in a.components.iter().zip(&b.components) {
        assert_eq!(ca.mode, cb.mode);
        assert_eq!(strip(&ca.geometry), strip(&cb.geometry));
    }
    // 4. And the WRITER discloses the same fact the reader did.
    let losses = mask_export_losses(&once);
    assert!(
        losses.iter().any(|l| l.reason == MaskLossReason::BrushRendered),
        "the writer must say the brush it emitted was drawn by us: {losses:?}"
    );
    assert!(
        !losses.iter().any(|l| l.reason == MaskLossReason::ComponentsFlattened),
        "nothing was flattened — the brush went out whole: {losses:?}"
    );
}

/// HAZARD 1 (`classify_correction` walked the mask block FLAT). A flat walk
/// sees the `Mask/Paint` strokes inside a group as SIBLINGS of it: they are
/// none of the four kinds the classifier knows, so each one sets
/// `unknown_component` and the whole correction is refused — the brush arm
/// would import nothing at all.
///
/// MUTATION-LINED: replacing the `components.iter().filter(|c| c.depth ==
/// 0)` walk with the old `next_xml_tag` loop over `mask_block` refuses this
/// document (`Unrepresentable`, 0 masks).
#[test]
fn nested_paint_strokes_are_not_siblings_of_their_group() {
    let doc = lr_doc(&lr_correction("Mask 1", "", &lr_brush_group("false", "")));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "a nested Paint must not read as an unknown component");
    let MaskGeometry::Brush { strokes, .. } = &r.masks[0].mask else { panic!() };
    // Counted ONCE each, as children — a flat walk would also have made
    // them top-level components and double-counted the strokes.
    assert_eq!(strokes.len(), 2);
    // And the depth filter is not a hiding place: a component nested inside
    // a container we do NOT model is markup this reader cannot account for,
    // so it refuses rather than walking past it.
    let smuggled = doc.replace(
        "<crs:Masks>",
        "<crs:Masks>\n<crs:Decoy><rdf:li crs:What=\"Mask/Gradient\" crs:ZeroX=\"0\" \
             crs:ZeroY=\"0\" crs:FullX=\"1\" crs:FullY=\"1\"/></crs:Decoy>",
    );
    assert_ne!(smuggled, doc, "the mutation did not apply");
    assert_eq!(
        xmp_to_recipe(&smuggled).masks.len(),
        0,
        "a component in an unmodelled container is markup we cannot account for"
    );
}

/// The F5 chain, end to end (R28 2b): an over-cap dab stream is
/// TRUNCATED, the truncation is DISCLOSED, and what we republish is still
/// a document our own reader accepts.
///
/// The construction is the adjudication's: 65,536 `"d 0 0"` tokens sit
/// exactly on the read side's token ceiling and pass it, then arrive at a
/// store-side cap counted in BYTES (393,215 vs 262,144). Before this
/// batch, `cap` cut that inside a token, `xmp_to_recipe` dropped the
/// `ClampSummary` on the floor so nothing said a word, and the writer —
/// whose split is the exact inverse of the reader's join — put the
/// fragment back into the sidecar, where our next read refused the whole
/// Aggregate and the group's masks vanished.
///
/// MUTATION THIS KILLS, three ways: revert `cap_tokens` to `cap` (a
/// republished `<rdf:li>d 0 </rdf:li>` fails `dab_token_is_known` below);
/// revert `xmp_to_recipe`'s tail to a bare `r.clamp();` (the summary is
/// empty and the disclosure assertion fails); drop the length bound in
/// `dab_token_is_known` (the single-huge-token arm at the end imports).
#[test]
fn an_oversized_dab_stream_is_disclosed_and_republished_whole_token_only() {
    let many = vec!["d 0 0"; 65_536];
    let big = lr_paint("2222222222222222222222222222222B", "1", "0", "false", &many);
    let doc = lr_doc(&lr_correction("Mask 1", "", &lr_brush_group("false", &big)));
    assert!(doc.len() < MAX_XMP_BYTES, "premise: the fixture is a readable document");

    let (r, clamped) = xmp_to_recipe_clamped(&doc);
    assert_eq!(r.masks.len(), 1, "premise: the brush group imports");
    // DISCLOSED, not swallowed — the single fact this whole item exists
    // for. `xmp_to_recipe`'s own clamp used to be the only one that saw
    // it, and it threw the answer away.
    assert!(
        clamped.truncated_string_bytes > 100_000,
        "the import cut ~131 KB of dabs and must say so: {clamped:?}"
    );

    // …and the projection we would hand back to Lightroom carries only
    // tokens of the measured grammar. `dab_token_is_known` is the reader's
    // own judge, so this asserts the round trip against the exact rule
    // that used to reject it.
    let out = recipe_to_xmp(&r);
    let mut checked = 0usize;
    let mut rest = out.as_str();
    while let Some(open) = rest.find("<crs:Dabs>") {
        let close = rest[open..].find("</crs:Dabs>").expect("closed Dabs block") + open;
        let mut seq = &rest[open..close];
        while let Some(i) = seq.find("<rdf:li>") {
            let j = seq[i..].find("</rdf:li>").expect("closed item") + i;
            let token = &seq[i + "<rdf:li>".len()..j];
            assert!(
                dab_token_is_known(token).is_ok(),
                "republished a token our own reader refuses: {token:?}"
            );
            checked += 1;
            seq = &seq[j..];
        }
        rest = &rest[close..];
    }
    assert!(checked > 40_000, "premise: the republished stream is the big one ({checked})");

    // The aggravator, same door: ONE token whose coordinate is `0.` plus
    // 300,000 digits parses to a finite `f32` (0.111…), passes every
    // shape check, and blows the byte cap by itself while the token COUNT
    // gate never fires. Refused at the token now — which, by this
    // reader's existing all-or-nothing rule for a group
    // (`parse_brush_group` propagates one bad Paint), refuses the
    // Aggregate and DISCLOSES it. That is the same verdict any other
    // malformed token already earns; the bound only stops the malformed
    // one from being called well-formed.
    //
    // FRACTIONAL, not the adjudication's 300,000 INTEGER digits: those
    // overflow to `inf` and the finiteness check already refused them, so
    // this is the shape that actually needed a length bound.
    let huge = format!("d 0 0.{}", "1".repeat(300_000));
    let mono = lr_paint("3333333333333333333333333333333C", "1", "0", "false", &[&huge]);
    let mono_doc = lr_doc(&lr_correction("Mask 1", "", &lr_brush_group("false", &mono)));
    let (mr, _) = xmp_to_recipe_clamped(&mono_doc);
    assert_eq!(mr.masks.len(), 0, "an unbounded token cannot import as a stroke");
    assert!(
        import_losses(&mono_doc).iter().any(|l| l.reason == MaskImportReason::OutOfModel),
        "and the refusal is named: {:?}",
        import_losses(&mono_doc)
    );
}

/// HAZARD 2 (`base_geometry_at` scanned the WHOLE correction segment for a
/// `crs:What="Mask/Gradient"` tag, nesting-blind). The correction here has
/// one real component — a SUBTRACT radial in `crs:CorrectionMasks` — and a
/// nested creative-Look block beside it holding a `Mask/Gradient` of its
/// own. F2 found that shape in the reference library: one of the 105
/// `Mask/Image` components lives inside a `crs:Preset`/`crs:Parameters`
/// block rather than in any correction's component list.
///
/// The old scan starts at byte 0 of the correction and takes the first
/// default-blend geometry tag it meets, which is the LOOK's gradient — so
/// the correction imported as a Linear mask built from a profile's baked
/// parameters, a shape the photographer never drew. The selector now
/// searches this correction's OWN component list, finds no default-blend
/// member there, and falls back to the subtract radial (some shape beats no
/// shape — see the function's doc).
///
/// MUTATION-LINED: reverting `base_geometry_at` to the old flat
/// `next_xml_tag` scan over `seg` imports the Look's gradient and this
/// fails on the geometry KIND.
#[test]
fn a_shape_nested_beside_the_component_list_is_never_the_corrections_base() {
    // A creative Look's baked parameters — owned-LOOKING crs markup that
    // belongs to the profile, not to this correction (the same trap
    // `top_level_owned_spans` documents for the merge).
    let look = concat!(
        "       <crs:Look>\n        <rdf:Description>\n         <crs:Parameters>\n",
        "         <rdf:Description>\n",
        "          <rdf:li crs:What=\"Mask/Gradient\" crs:MaskActive=\"true\"\n",
        "           crs:MaskBlendMode=\"0\" crs:MaskInverted=\"false\" crs:MaskValue=\"1\"\n",
        "           crs:ZeroX=\"0.9\" crs:ZeroY=\"0.9\" crs:FullX=\"0.1\" crs:FullY=\"0.1\"/>\n",
        "         </rdf:Description>\n        </crs:Parameters>\n",
        "        </rdf:Description>\n       </crs:Look>\n",
    );
    let doc =
        lr_doc(&lr_correction_with_curves("Mask 1", "", look, &lr_radial("0", "1")));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "the correction still imports: {:?}", r.masks);
    assert!(
        matches!(r.masks[0].mask, MaskGeometry::Radial { .. }),
        "the base must come from this correction's OWN component list, never from a \
             nested Look: {:?}",
        r.masks[0].mask
    );
}

/// The other half of the same rule, and the one the brush arm needs: a
/// parametric shape nested INSIDE a brush group is refused rather than
/// promoted. "An Aggregate whose child is not a Paint" has zero
/// counter-examples in 177 current sidecars, so a document with one was
/// written by something other than Lightroom.
///
/// MUTATION-LINED: loosening `parse_brush_group`'s child-kind gate from
/// `return Err(())` to `continue` imports the correction.
#[test]
fn a_shape_nested_inside_a_brush_group_is_refused_not_promoted() {
    let nested = "<rdf:li crs:What=\"Mask/Gradient\" crs:MaskActive=\"true\" \
                      crs:MaskBlendMode=\"0\" crs:MaskInverted=\"false\" crs:MaskValue=\"1\" \
                      crs:ZeroX=\"0.1\" crs:ZeroY=\"0.2\" crs:FullX=\"0.3\" \
                      crs:FullY=\"0.4\"/>\n";
    let doc = lr_doc(&lr_correction("Mask 1", "", &lr_brush_group("false", nested)));
    let r = xmp_to_recipe(&doc);
    assert!(
        r.masks.is_empty(),
        "a gradient inside an Aggregate is a shape Lightroom never writes — refuse it, \
             do not promote it: {:?}",
        r.masks
    );
    assert!(
        import_losses(&doc).iter().any(|l| l.reason == MaskImportReason::OutOfModel),
        "and say which kind of refusal it was — the NESTING is accounted for, it is the \
             shape that is outside the model"
    );
}

/// HAZARD 3 (`parse_one_correction` read geometry keys from a slice running
/// to the END of the correction). The base gradient here omits
/// `crs:MaskInverted`; the brush group AFTER it carries
/// `crs:MaskInverted="true"`. The unbounded scan finds the GROUP's bit and
/// inverts a mask the base never asked to invert.
///
/// MUTATION-LINED: changing `base_element(seg, p)` back to `&seg[p..]`
/// makes `inverted` read `true` and the first assertion fails.
#[test]
fn a_later_components_attribute_cannot_answer_for_the_base_shape() {
    let bare = lr_gradient("0").replace("crs:MaskInverted=\"false\"\n", "");
    assert!(!bare.contains("MaskInverted"), "the base must declare no inversion");
    let group = lr_brush_group("true", "");
    let doc = lr_doc(&lr_correction("Mask 7", "", &format!("{bare}{group}")));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "{:?}", r.masks);
    assert!(
        !r.masks[0].inverted,
        "the base gradient declares no inversion — the group's bit is the GROUP's"
    );
    // And the group keeps its own bit, carried where it belongs.
    let MaskGeometry::Brush { inverted, .. } = &r.masks[0].components[0].geometry else {
        panic!("expected the group as a component")
    };
    assert!(*inverted, "the Aggregate's own MaskInverted rides in the geometry");
}

/// The measured INVARIANTS are gates, not fields: a `Mask/Paint` that
/// asserts a composition, a missing attribute, a dab token outside
/// `{r,d,f,h}`, a Paint with no `crs:Dabs`. Each has zero counter-examples
/// in 177 current sidecars, so each costs the correction rather than being
/// guessed past — the roundness rule, applied to a stroke.
///
/// MUTATION-LINED: loosening any one gate in `parse_brush_group` /
/// `parse_paint_stroke` / `dab_token_is_known` imports the corresponding
/// document and fails the matching assertion.
#[test]
fn a_brush_group_outside_the_measured_encoding_is_refused_not_guessed() {
    let base = lr_doc(&lr_correction("Mask 1", "", &lr_brush_group("false", "")));
    assert_eq!(xmp_to_recipe(&base).masks.len(), 1, "the control must import");
    for (what, doc) in [
        (
            "a Paint asserting its own blend mode",
            base.replace(
                "crs:What=\"Mask/Paint\" crs:MaskActive=\"true\"\ncrs:MaskBlendMode=\"0\"",
                "crs:What=\"Mask/Paint\" crs:MaskActive=\"true\"\ncrs:MaskBlendMode=\"1\"",
            ),
        ),
        (
            "a Paint that inverts itself",
            base.replace(
                "crs:MaskBlendMode=\"0\" crs:MaskInverted=\"false\"",
                "crs:MaskBlendMode=\"0\" crs:MaskInverted=\"true\"",
            ),
        ),
        (
            "a Paint missing one of its nine attributes",
            base.replace(" crs:Flow=\"1\"", ""),
        ),
        (
            "a dab token of an unknown form",
            base.replace("<rdf:li>r 0.581835</rdf:li>", "<rdf:li>q 0.581835</rdf:li>"),
        ),
        (
            "a dab token of the wrong arity",
            base.replace("<rdf:li>r 0.581835</rdf:li>", "<rdf:li>r 0.5 0.6</rdf:li>"),
        ),
        (
            "a dab coordinate that is not a number",
            base.replace(
                "<rdf:li>d 0.000684 0.940004</rdf:li>",
                "<rdf:li>d 0.000684 nine</rdf:li>",
            ),
        ),
        (
            "a Paint with no Dabs at all",
            base.replace("crs:Dabs>", "crs:NotDabs>"),
        ),
        (
            "an Aggregate with no strokes at all",
            lr_doc(&lr_correction(
                "Mask 1",
                "",
                "<rdf:li>\n<rdf:Description crs:What=\"Mask/Aggregate\" \
                     crs:MaskActive=\"true\" crs:MaskName=\"Brush 1\" crs:MaskBlendMode=\"0\" \
                     crs:MaskInverted=\"false\" crs:MaskValue=\"1\">\n<crs:Masks>\n\
                     <rdf:Seq>\n</rdf:Seq>\n</crs:Masks>\n</rdf:Description>\n</rdf:li>\n",
            )),
        ),
    ] {
        assert_ne!(doc, base, "the mutation for {what:?} did not apply");
        assert!(
            xmp_to_recipe(&doc).masks.is_empty(),
            "{what} must refuse the correction, not be imported as if understood"
        );
    }
}

/// A brush group is DRAWN by our own rasteriser and NAMED in both
/// disclosure channels — it is neither passed off as Adobe's alpha nor
/// silently approximated.
///
/// R29 Batch-6b rewrote this from `a_carried_brush_is_named_…`: the phrase
/// it used to require ("carried" + "not yet rendered") was the disclosure
/// of an engine that drew nothing, and keeping it green would have meant
/// shipping a sentence the renderer had stopped honouring.
///
/// MUTATION-LINED: reverting either `en()` to the old
/// 「carried, not yet rendered」wording fails the phrase asserts below, and
/// dropping either variant from `ALL` fails the first two lines — the lists
/// every disclosure surface iterates.
#[test]
fn a_rendered_brush_is_named_in_both_channels_and_is_not_a_drop() {
    // Import twin and export twin describe the SAME fact, so both `ALL`
    // arrays — the lists every disclosure surface iterates — must hold it.
    assert!(MaskImportReason::ALL.contains(&MaskImportReason::BrushRendered));
    assert!(MaskLossReason::ALL.contains(&MaskLossReason::BrushRendered));
    assert!(!MaskImportReason::BrushRendered.is_drop(), "the correction DID import");
    for phrase in [MaskImportReason::BrushRendered.en(), MaskLossReason::BrushRendered.en()] {
        // Both halves of the sentence, because either one alone misleads:
        // "drawn" without "not Adobe's" reads as a raster round trip, and
        // "not Adobe's" without "drawn" reads as the old refusal.
        assert!(phrase.contains("drawn"), "{phrase}");
        assert!(phrase.contains("measured model"), "{phrase}");
        assert!(phrase.contains("not Adobe's own rasteriser"), "{phrase}");
        assert!(!phrase.contains("not yet rendered"), "the old refusal wording: {phrase}");
    }
    // The RENDER half of the same claim lives in render.rs, where
    // `mask_weight` is: `a_carried_brush_group_draws_its_dabs`.
}

/// FORENSIC REGRESSION, run against the user's own Lightroom library.
/// The inline fixtures above are synthetic by policy, which means they
/// prove the RULES and not the FILES — and §0 of this round was a defect
/// nobody's synthetic fixture had caught in four releases.
///
/// Point `AUTOSHADE_MB_FIXTURES` at a directory of `.xmp` / `.xmp.txt`
/// sidecars and this asserts, per file, that every `crs:What="Correction"`
/// is accounted for (imported + refused) and that the parametric ones
/// really do arrive — which is 0 on every one of them before this batch.
/// Unset, it is a silent no-op: the reference files are photographs, they
/// are not in this repository, and no path to them appears in this test.
#[test]
fn real_lightroom_sidecars_import_their_parametric_masks() {
    let Some(dir) = crate::config::live_env("AUTOSHADE_MB_FIXTURES") else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("AUTOSHADE_MB_FIXTURES is set but unreadable: {dir}");
    };
    let mut files = 0usize;
    let mut total_imported = 0usize;
    for e in entries.flatten() {
        let p = e.path();
        if !p.to_string_lossy().to_lowercase().contains(".xmp") {
            continue;
        }
        // NAMED, never skipped (R25 P8). `else { continue }` here meant a
        // sidecar this probe could not read simply left the count — and a
        // forensic probe whose files quietly stop arriving is a green
        // test that measures nothing. A `.xmp` in the fixture directory
        // that will not read as UTF-8 is a fact about the fixtures the
        // round report has to hear.
        let text = std::fs::read_to_string(&p)
            .unwrap_or_else(|e| panic!("{}: fixture unreadable ({e})", p.display()));
        files += 1;
        let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        // The block's OWN corrections, counted the way the reader scopes
        // them — retouch areas and creative Looks carry `Mask/*`
        // components of their own and are not corrections.
        let block = crs_own_scope(&text);
        let corrections = owned_element_body(block.as_ref(), "crs:MaskGroupBasedCorrections")
            .ok()
            .flatten()
            .map(|b| b.matches("crs:What=\"Correction\"").count())
            .unwrap_or(0);
        let t0 = std::time::Instant::now();
        let imported = xmp_to_recipe(&text).masks.len();
        let import_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let refused = unsupported_corrections(&text);
        let losses = import_losses(&text);
        eprintln!(
            "{name}: {corrections} correction(s) → {imported} imported, {refused} refused, \
                 {} loss note(s), parse {import_ms:.2} ms",
            losses.len()
        );
        // The forensic half: WHICH verdict landed on which correction.
        // This is the output the probe exists for — a count says the
        // import worked, this says whether it was right.
        for l in &losses {
            eprintln!("    {:?}  {}", l.reason, l.name);
        }
        // R25 P5: the two carried radial attributes, observed on the real
        // files and then round-tripped through OUR writer. The assertion
        // is the invariant that matters — a value we do not interpret must
        // come back exactly as it went in — and the print is the evidence
        // for the round report (before this batch every radial read 50/2
        // because neither attribute was looked at).
        let mine = xmp_to_recipe(&recipe_to_xmp(&xmp_to_recipe(&text)));
        for (i, m) in xmp_to_recipe(&text).masks.iter().enumerate() {
            let (
                MaskGeometry::Radial { midpoint, mask_version, .. },
                Some(LocalAdjustment {
                    mask: MaskGeometry::Radial {
                        midpoint: rt_mid, mask_version: rt_ver, ..
                    },
                    ..
                }),
            ) = (&m.mask, mine.masks.get(i))
            else {
                continue;
            };
            eprintln!("    radial {i}: Midpoint={midpoint} Version={mask_version}");
            assert_eq!(
                (midpoint, mask_version),
                (rt_mid, rt_ver),
                "{name}: radial {i} lost a carried attribute in the round-trip"
            );
        }
        // R25 P6: the four LOCAL point curves, per correction. 19 files in
        // the user's library carry 43 of them and every one used to be
        // dropped with a note; the print is the round report's per-file
        // curve list and the assertion is the round trip through OUR
        // writer — the one place the `x,y` spelling could silently drift
        // to the global `x, y` and still look right in a diff.
        let imported_recipe = xmp_to_recipe(&text);
        for (i, (m, rt)) in imported_recipe.masks.iter().zip(&mine.masks).enumerate() {
            for (key, got, round) in [
                ("MainCurve", &m.main_curve, &rt.main_curve),
                ("RedCurve", &m.red_curve, &rt.red_curve),
                ("GreenCurve", &m.green_curve, &rt.green_curve),
                ("BlueCurve", &m.blue_curve, &rt.blue_curve),
            ] {
                if got.is_empty() {
                    continue;
                }
                let pts: Vec<String> =
                    got.iter().map(|p| format!("{},{}", p.input, p.output)).collect();
                eprintln!(
                    "    mask {i} crs:{key}: {} point(s) [{}]",
                    got.len(),
                    pts.join(" ")
                );
                assert_eq!(got, round, "{name}: mask {i} lost crs:{key} in the round-trip");
            }
        }
        assert_eq!(
            imported + refused,
            corrections,
            "{name}: every correction must be either imported or counted as refused"
        );
        // GATED on the sidecar actually HAVING a correction (R27 Batch-4).
        // The bare `imported > 0` was true of the seven M-B fixtures and
        // false of the assertion's own sentence: pointed at any real
        // catalogue folder it failed on the first sidecar carrying nothing
        // but global sliders, claiming a file "with 0 correction(s) must
        // import at least one". A probe that cannot be aimed at a
        // directory of real photographs is a probe that only ever sees the
        // seven files someone already curated.
        assert!(
            corrections == 0 || imported > 0,
            "{name}: a real Lightroom sidecar with {corrections} correction(s) must import \
                 at least one — importing none is the defect this batch closed"
        );
        total_imported += imported;
    }
    assert!(files > 0, "AUTOSHADE_MB_FIXTURES held no sidecars: {dir}");
    eprintln!("{files} sidecar(s), {total_imported} mask(s) imported in total");
}

/// FORENSIC REGRESSION for R25 P9, on the real files — the census this
/// batch's fix rests on, re-derived from the bytes every time it runs
/// rather than quoted from a document. Same directory and same silent-skip
/// rule as the two probes around it.
///
/// Three claims, in order of how much they cost if false:
///  1. `crs:Flipped` is the COMPLEMENT of `crs:MaskInverted` on every
///     radial in the fixtures (16 `(true,false)` + 7 `(false,true)` = 23/23
///     across the 7 M-B sidecars, matching 201/201 over the whole library).
///     A fixture set that ever shows a MATCHING pair falsifies the model
///     this batch is built on, and this test is where that would surface.
///  2. No imported radial carries `flipped` — the inversion is read from
///     `crs:MaskInverted` alone. Before this batch the two were XORed and
///     the net came out `true` on every radial in every one of these files
///     (asserted below as the anti-regression: `!net_before == net_now`
///     would have to hold for the 16, which it does not).
///  3. Our writer hands the file its own pair back, attribute for
///     attribute — so a Lightroom → AutoShade → Lightroom trip renders the
///     same mask at both ends.
#[test]
fn real_lightroom_radials_carry_one_inversion_bit_spelled_twice() {
    let Some(dir) = crate::config::live_env("AUTOSHADE_MB_FIXTURES") else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("AUTOSHADE_MB_FIXTURES is set but unreadable: {dir}");
    };
    let (mut files, mut radials) = (0usize, 0usize);
    let (mut flip_true, mut flip_false) = (0usize, 0usize);
    for e in entries.flatten() {
        let p = e.path();
        if !p.to_string_lossy().to_lowercase().contains(".xmp") {
            continue;
        }
        let text = std::fs::read_to_string(&p)
            .unwrap_or_else(|e| panic!("{}: fixture unreadable ({e})", p.display()));
        files += 1;
        let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        // Scoped the way the importer scopes: this document's OWN
        // corrections. `crs:RetouchAreas` carries `Mask/*` components of
        // its own and is not a correction.
        let scope = crs_own_scope(&text);
        let block = owned_element_body(scope.as_ref(), "crs:MaskGroupBasedCorrections")
            .ok()
            .flatten()
            .unwrap_or_default();
        // CLAIM 1 — the census, on this file's bytes.
        let mut at = 0usize;
        let mut per_file = 0usize;
        while let Some((s, e, _)) = next_xml_tag(block, at) {
            at = e + 1;
            let tag = &block[s..=e];
            if xml_attribute_raw(tag, "crs:What").map(|(_, v)| xml_unescape(v))
                != Some("Mask/CircularGradient".into())
            {
                continue;
            }
            let tag = Tag::new(tag);
            let f = tag.crs_str("Flipped").map(|v| v.as_ref() == "true");
            let i = tag.crs_str("MaskInverted").map(|v| v.as_ref() == "true");
            let (Some(f), Some(i)) = (f, i) else {
                panic!("{name}: a radial without both flags — {f:?} / {i:?}");
            };
            assert_ne!(
                f, i,
                "{name}: radial {per_file} carries Flipped={f} MaskInverted={i} — a MATCHING \
                     pair, which no radial in the 201-mask library census does. The one-bit model \
                     R25 P9 is built on does not hold on this file; do not paper over it."
            );
            if f { flip_true += 1 } else { flip_false += 1 }
            per_file += 1;
            radials += 1;
        }
        // CLAIMS 2 and 3 — what the importer and the writer do with them.
        let imported = xmp_to_recipe(&text);
        let round = xmp_to_recipe(&recipe_to_xmp(&imported));
        let mut seen = 0usize;
        for (i, m) in imported.masks.iter().enumerate() {
            let MaskGeometry::Radial { flipped, .. } = m.mask else { continue };
            assert!(
                !flipped,
                "{name}: mask {i} imported flipped — crs:Flipped reached the render flag"
            );
            let rt = round.masks.get(i).unwrap_or_else(|| panic!("{name}: mask {i} vanished"));
            assert_eq!(
                lr_net_inverted(m),
                lr_net_inverted(rt),
                "{name}: mask {i} changed its inversion in the round trip"
            );
            seen += 1;
        }
        eprintln!(
            "{name}: {per_file} radial(s) in the file, {seen} imported, all anti-correlated"
        );
    }
    assert!(files > 0, "AUTOSHADE_MB_FIXTURES held no sidecars: {dir}");
    assert!(radials > 0, "no radial reached the census: {dir}");
    // The anti-regression, stated as the arithmetic that made the defect
    // visible: XORing the two flags gives `true` on EVERY radial here,
    // whatever the file says, because they are complements. That is what
    // the old importer did, and why it inverted the `Flipped=true` ones.
    eprintln!(
        "{radials} radial(s): {flip_true} Flipped=true (Lightroom does NOT invert these — \
             the {flip_true} the old importer inverted), {flip_false} Flipped=false"
    );
    assert_eq!(flip_true + flip_false, radials);
    assert!(
        flip_true > 0,
        "the fixtures hold no NOT-inverted radial, so they cannot witness the defect"
    );
}

/// FORENSIC REGRESSION for the B2 GLOBALS, same directory and same
/// silent-skip rule as the mask probe above (the reference files are
/// photographs and are not in this repository; the inline fixtures beside
/// this one are synthetic by policy, so they prove the RULES and not the
/// FILES).
///
/// `P34.xmp` is the strongest case in the user's library: global
/// Texture +26 — the largest of the seven — beside a real post-crop
/// vignette. Before B2 every one of those values imported as zero and the
/// photo simply rendered differently from Lightroom.
#[test]
fn real_lightroom_sidecars_import_their_global_effects() {
    let Some(dir) = crate::config::live_env("AUTOSHADE_MB_FIXTURES") else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("AUTOSHADE_MB_FIXTURES is set but unreadable: {dir}");
    };
    let mut seen_texture = 0usize;
    let mut seen_effects = 0usize;
    let mut seen_auto_ca = 0usize;
    for e in entries.flatten() {
        let p = e.path();
        if !p.to_string_lossy().to_lowercase().contains(".xmp") {
            continue;
        }
        // NAMED, never skipped (R25 P8). `else { continue }` here meant a
        // sidecar this probe could not read simply left the count — and a
        // forensic probe whose files quietly stop arriving is a green
        // test that measures nothing. A `.xmp` in the fixture directory
        // that will not read as UTF-8 is a fact about the fixtures the
        // round report has to hear.
        let text = std::fs::read_to_string(&p)
            .unwrap_or_else(|e| panic!("{}: fixture unreadable ({e})", p.display()));
        let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        let r = xmp_to_recipe(&text);
        eprintln!(
            "{name}: texture {} · post-crop vignette {}/{}/{}/{}/{}/{} · grain {}/{}/{}",
            r.texture,
            r.post_crop_vignette,
            r.post_crop_vignette_mid,
            r.post_crop_vignette_feather,
            r.post_crop_vignette_round,
            r.post_crop_vignette_style,
            r.post_crop_vignette_hl,
            r.grain,
            r.grain_size,
            r.grain_rough,
        );
        // The B3 block, same forensic line: what the eight detail axes,
        // the CA pair and the six de-fringe keys actually came back as.
        // `sharpening` leads it since v0.31.1 — the ×1.5 that used to sit
        // on this read was the batch's headline defect, and the value it
        // produces is the number this probe exists to show. Six of the
        // seven reference files carry `crs:Sharpness="40"` and one carries
        // `"35"`; before the fix they imported as 60 and 52.5.
        eprintln!(
            "    sharpening {} · detail {}/{}/{} · nr {}/{} · colour nr {}/{}/{} · \
                 ca {}/{} auto {} · defringe {}/{}/{} {}/{}/{}",
            r.sharpening,
            r.sharpen_radius,
            r.sharpen_detail,
            r.sharpen_mask,
            r.nr_detail,
            r.nr_contrast,
            r.color_nr,
            r.color_nr_detail,
            r.color_nr_smooth,
            r.ca_r,
            r.ca_b,
            r.auto_lateral_ca,
            r.defringe_purple,
            r.defringe_purple_lo,
            r.defringe_purple_hi,
            r.defringe_green,
            r.defringe_green_lo,
            r.defringe_green_hi,
        );
        // The de-fringe block is the ONE with a non-zero neutral, and
        // every file in this library carries it at Adobe's defaults — so
        // the fallback is exercised on real bytes here, not only on the
        // synthetic fixtures. A reader that took `crs_f32`'s absent-key
        // zero would land on 0/0 and fail this on all seven.
        assert_eq!(
            (
                r.defringe_purple_lo,
                r.defringe_purple_hi,
                r.defringe_green_lo,
                r.defringe_green_hi
            ),
            (30.0, 70.0, 40.0, 60.0),
            "{name}: the real de-fringe hue windows must import as themselves"
        );
        // Two named forensic cases from the first-hand scan of these
        // files: every one writes `SharpenRadius="+1.0"`, and P31 is
        // the only one whose auto-CA switch is on.
        assert_eq!(r.sharpen_radius, 1.0, "{name}: crs:SharpenRadius=\"+1.0\" must import as 1.0");
        if name.starts_with("P31") {
            assert!(r.auto_lateral_ca, "{name}: crs:AutoLateralCA=\"1\" must import as on");
            seen_auto_ca += 1;
        }
        // The named case, asserted exactly.
        if name.starts_with("P34") {
            assert_eq!(r.texture, 26.0, "{name}: crs:Texture=\"+26\" must import as 26");
            assert_eq!(r.post_crop_vignette, -17.0, "{name}: its post-crop vignette too");
            assert_eq!(r.post_crop_vignette_style, 1.0, "{name}: Highlight Priority");
            seen_texture += 1;
        }
        if r.texture != 0.0 || r.post_crop_vignette != 0.0 || r.grain != 0.0 {
            seen_effects += 1;
        }
        // Whatever the values are, they must SURVIVE a save: the merge
        // strips these keys now, so a read/write asymmetry would delete
        // them from the file beside the RAW.
        if let Some(merged) = merge_recipe_into_xmp(&text, &r) {
            let round = xmp_to_recipe(&merged.doc);
            assert_eq!(round.texture, r.texture, "{name}: Texture lost on merge");
            assert_eq!(
                (round.post_crop_vignette, round.grain),
                (r.post_crop_vignette, r.grain),
                "{name}: a carried effect was lost on merge"
            );
            assert_eq!(
                (round.sharpen_radius, round.color_nr, round.auto_lateral_ca),
                (r.sharpen_radius, r.color_nr, r.auto_lateral_ca),
                "{name}: a B3 carried detail value was lost on merge"
            );
            assert_eq!(
                (round.defringe_purple_lo, round.defringe_green_hi),
                (r.defringe_purple_lo, r.defringe_green_hi),
                "{name}: the de-fringe hue windows were lost on merge"
            );
        }
    }
    assert!(
        seen_texture > 0,
        "P34.xmp was not in {dir} — the named forensic case never ran"
    );
    assert!(
        seen_auto_ca > 0,
        "P31.xmp was not in {dir} — the B3 auto-CA forensic case never ran"
    );
    eprintln!("{seen_effects} sidecar(s) carried a non-neutral B2 effect");
}

/// FORENSIC REGRESSION for **the R25 P8 root cause**, on the seven real
/// sidecars and in the exact shape the defect takes in the field: a v0.30
/// `recipe.json` (no `schema_era`, no field for any of the twenty-seven
/// R25 keys or any v1.5.0 control, no masks — that build could not
/// import one) saved back over the Lightroom file it came from.
///
/// Same directory and same silent-skip rule as the probes above. This is
/// where the numbers in the round report come from: before the fix, four
/// corrections were destroyed on P31, eight on P51, and nine
/// global keys on each of the three files that carry them — every one of
/// them silently, with an empty note list.
#[test]
fn real_lightroom_sidecars_survive_a_v0_30_recipe() {
    let Some(dir) = crate::config::live_env("AUTOSHADE_MB_FIXTURES") else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("AUTOSHADE_MB_FIXTURES is set but unreadable: {dir}");
    };
    let (mut files, mut masks_held, mut keys_held) = (0usize, 0usize, 0usize);
    for e in entries.flatten() {
        let p = e.path();
        if !p.to_string_lossy().to_lowercase().contains(".xmp") {
            continue;
        }
        let text = std::fs::read_to_string(&p)
            .unwrap_or_else(|e| panic!("{}: fixture unreadable ({e})", p.display()));
        files += 1;
        let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        let live = xmp_to_recipe(&text);
        let legacy = EditRecipe { masks: Vec::new(), ..as_v0_30_recipe(&live) };
        assert_eq!(legacy.schema_era, 0);
        let Some(out) = merge_recipe_into_xmp(&text, &legacy) else {
            panic!("{name}: the reference sidecars are all mergeable");
        };

        // 1) The mask block, byte for byte.
        let block = text
            .find("<crs:MaskGroupBasedCorrections>")
            .zip(text.find("</crs:MaskGroupBasedCorrections>"))
            .map(|(s, e)| &text[s..e + "</crs:MaskGroupBasedCorrections>".len()]);
        if let Some(original) = block {
            let corrections = original.matches("crs:What=\"Correction\"").count();
            assert!(
                out.doc.contains(original),
                "{name}: {corrections} correction(s) did not survive a v0.30 save"
            );
            masks_held += corrections;
        }

        // 2) Every key of every era the recipe never had, in BOTH the
        //    document's terms and the engine's.
        //
        //    The document's first, because that is the promise: the
        //    attribute the file arrived with is still there, spelled
        //    exactly as Lightroom spelled it, exactly once.
        //
        //    Then the read-back recipe — but a COMPANION control
        //    (`recipe::LR_COMPANION_DEFAULTS`) through `resolved`, because
        //    its stored 0 and Lightroom's own default for it are ONE
        //    render, and the payload reconciliation collapses them on
        //    purpose: a key this writer omits at rest, read back at Camera
        //    Raw's default, is a materialisation and not an edit
        //    (`payload::lightroom_materialised`). Comparing the stored
        //    number instead called an untouched `crs:SharpenRadius="+1.0"`
        //    a change while the document held it verbatim — the assertion
        //    was wrong about the engine, not the engine about the file.
        let round = xmp_to_recipe(&out.doc);
        let attr_value = |doc: &str, spelt: &str| {
            doc.find(spelt).map(|i| {
                let rest = &doc[i + spelt.len()..];
                rest[..rest.find('"').unwrap_or(0)].to_string()
            })
        };
        for (control, key) in (1..=crate::recipe::SCHEMA_ERA).flat_map(era_attr_keys) {
            let spelt = format!("crs:{key}=\"");
            if let Some(before) = attr_value(&text, &spelt) {
                assert_eq!(
                    Some(&before),
                    attr_value(&out.doc, &spelt).as_ref(),
                    "{name}: crs:{key} changed on a v0.30 save"
                );
                assert_eq!(
                    out.doc.matches(&spelt).count(),
                    1,
                    "{name}: crs:{key} must appear exactly once"
                );
                keys_held += 1;
            }
            if crate::recipe::LR_COMPANION_DEFAULTS.iter().any(|(n, _)| *n == control) {
                assert_eq!(
                    live.resolved(control),
                    round.resolved(control),
                    "{name}: crs:{key} renders differently after a v0.30 save"
                );
            } else {
                assert_eq!(
                    crate::advisor::catalogue::global_value(&live, control),
                    crate::advisor::catalogue::global_value(&round, control),
                    "{name}: crs:{key} changed on a v0.30 save"
                );
            }
        }
        // 3) …and none of it is a silent success by way of an empty file.
        assert!(out.notes.is_empty(), "{name}: nothing was replaced: {:?}", out.notes);
    }
    assert!(files > 0, "AUTOSHADE_MB_FIXTURES held no sidecars: {dir}");
    eprintln!(
        "{files} sidecar(s): {masks_held} correction(s) and {keys_held} era-gated key(s) held \
             through a v0.30-shaped save"
    );
}

/// FORENSIC REGRESSION for the B4 PASS-THROUGH blocks, same directory and
/// same silent-skip rule as the two probes above.
///
/// This is the one batch whose whole promise is "the bytes come back",
/// and synthetic fixtures cannot prove it: the spellings are the point,
/// and only Lightroom writes them. First-hand from the seven reference
/// sidecars — `CameraProfile` on every file, the six Upright bookkeeping
/// keys on exactly one, and NOT ONE of the `CameraCalibration*` keys R25
/// once listed here (Lightroom's Calibration block is the unprefixed
/// `crs:ShadowTint` / `crs:BlueHue` / … one, on all seven, which v1.5.0
/// renders) — which is exactly why an absent key must stay absent instead
/// of being invented at some neutral we chose.
///
/// v1.5.0 F6 REVISION. The eight `crs:Perspective*` keys used to be this
/// probe's whole subject and are owned controls now, so the counts below
/// moved with them: the probe asserts they reach their own FIELDS on every
/// file, and counts the bookkeeping that is still carried. That split is
/// the fact the forensic set is uniquely able to state — synthetic bytes
/// prove a parser, only Lightroom's own files prove which keys it writes.
#[test]
fn real_lightroom_sidecars_pass_their_transform_blocks_through() {
    let Some(dir) = crate::config::live_env("AUTOSHADE_MB_FIXTURES") else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("AUTOSHADE_MB_FIXTURES is set but unreadable: {dir}");
    };
    let mut files = 0usize;
    let mut seen_profile = 0usize;
    let mut seen_upright = 0usize;
    let mut seen_sliders = 0usize;
    for e in entries.flatten() {
        let p = e.path();
        if !p.to_string_lossy().to_lowercase().contains(".xmp") {
            continue;
        }
        // NAMED, never skipped (R25 P8). `else { continue }` here meant a
        // sidecar this probe could not read simply left the count — and a
        // forensic probe whose files quietly stop arriving is a green
        // test that measures nothing. A `.xmp` in the fixture directory
        // that will not read as UTF-8 is a fact about the fixtures the
        // round report has to hear.
        let text = std::fs::read_to_string(&p)
            .unwrap_or_else(|e| panic!("{}: fixture unreadable ({e})", p.display()));
        files += 1;
        let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        let r = xmp_to_recipe(&text);
        let shown: Vec<String> =
            PASSTHROUGH_CRS.iter().filter_map(|k| r.passthrough.get(*k).map(|v| format!("{k}={v:?}"))).collect();
        eprintln!("{name}: {} passthrough key(s) — {}", shown.len(), shown.join(" "));
        if r.passthrough.contains_key("CameraProfile") {
            seen_profile += 1;
        }
        if r.passthrough.contains_key("UprightVersion") {
            assert_eq!(
                PASSTHROUGH_CRS.iter().filter(|k| r.passthrough.contains_key(**k)).count(),
                7,
                "{name}: Lightroom writes the solver's bookkeeping as a block, all or none"
            );
            seen_upright += 1;
        }
        // The other half of the same measurement, and the half this batch
        // moved: the Perspective block is on every file, and it now lands
        // in the recipe's own fields rather than in the carried map. The
        // resting document reads as all-neutral, which is the value that
        // would be indistinguishable from "never parsed" — so the ONE file
        // that is not at rest is what carries the assertion.
        let block = [
            r.perspective_vertical, r.perspective_horizontal, r.perspective_rotate,
            r.perspective_aspect, r.perspective_x, r.perspective_y,
        ];
        assert_eq!(r.perspective_scale, 100.0, "{name}: 100 is the Transform panel's rest");
        if block.iter().any(|v| *v != 0.0) {
            seen_sliders += 1;
            eprintln!("{name}: manual Transform sliders {block:?}");
        }
        for k in ["PerspectiveUpright", "PerspectiveVertical", "PerspectiveScale"] {
            assert!(
                !r.passthrough.contains_key(k),
                "{name}: crs:{k} is an owned control since v1.5.0 F6, not a carried string"
            );
        }
        // VERBATIM, on real bytes: every value that arrived must reach the
        // merged document as the identical string, and come back as the
        // identical string. A formatter anywhere in this path would show
        // up here as `+0.9` → `0.9` or `0.00` → `0`.
        //
        // Counted inside `crs_own_scope`, and this probe is what proved
        // the distinction matters: every one of the reference sidecars
        // carries a SECOND `crs:CameraProfile` inside its creative Look's
        // baked `<crs:Parameters>`, which the merge preserves on purpose
        // (`top_level_owned_spans` strips top-level properties only) and
        // the reader is already scoped away from. A flat count over the
        // whole document reports two and is reading someone else's
        // settings block as our duplicate.
        if let Some(merged) = merge_recipe_into_xmp(&text, &r) {
            let scope = crs_own_scope(&merged.doc);
            for (k, v) in &r.passthrough {
                assert!(
                    scope.contains(&format!("crs:{k}=\"{v}\"")),
                    "{name}: crs:{k} did not reach the merged document as {v:?}"
                );
                assert_eq!(
                    scope.matches(&format!("crs:{k}=")).count(),
                    1,
                    "{name}: crs:{k} was written twice — the strip missed the original"
                );
            }
            assert_eq!(
                xmp_to_recipe(&merged.doc).passthrough,
                r.passthrough,
                "{name}: the pass-through block did not survive its own round trip"
            );
        }
    }
    assert!(files > 0, "AUTOSHADE_MB_FIXTURES held no sidecars: {dir}");
    assert_eq!(seen_profile, files, "every reference sidecar carries crs:CameraProfile");
    // Measured, not assumed: Lightroom writes the Upright solver's
    // bookkeeping only where its panel has actually run. One file of the
    // seven — the same one that carries the manual keystone below.
    assert_eq!(seen_upright, 1, "one reference sidecar carries the Upright bookkeeping");
    assert_eq!(seen_sliders, 1, "…and exactly one has its Transform sliders off neutral");
}
