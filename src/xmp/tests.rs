use super::*;

#[test]
fn atmosphere_xmp_contains_only_representable_global_controls() {
    let recipe = EditRecipe {
        exposure_ev: -0.8,
        temperature_k: Some(9000.0),
        tint: 10.0,
        saturation: 30.0,
        tone_curve: vec![
            CurvePoint { input: 0, output: 0 },
            CurvePoint { input: 64, output: 48 },
            CurvePoint { input: 128, output: 112 },
            CurvePoint { input: 192, output: 208 },
            CurvePoint { input: 255, output: 255 },
        ],
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Bitmap { path: "atmosphere-sky.png".into() },
            role: crate::recipe::MaskRole::ZoneSky,
            exposure_ev: -0.5,
            color_gains: Some([1.18, 0.96, 0.85]),
            saturation: -20.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let (doc, losses) = recipe_to_xmp_with_losses(&recipe);
    assert!(doc.contains(r#"crs:Exposure2012="-0.80""#));
    assert!(doc.contains(r#"crs:Temperature="9000""#));
    assert!(doc.contains(r#"crs:Tint="+10""#));
    assert!(doc.contains(r#"crs:Saturation="+30""#));
    assert!(doc.contains("crs:ToneCurvePV2012"));
    assert!(!doc.contains("ToneCurvePV2012Red"));
    assert!(!doc.contains("ToneCurvePV2012Green"));
    assert!(!doc.contains("ToneCurvePV2012Blue"));
    assert!(!doc.contains("MaskGroupBasedCorrections"));
    // TWO verdicts about the one mask (v1.3.1): the projection skips it,
    // and the payload could not embed a raster this test never wrote.
    assert_eq!(
        losses.iter().filter(|l| l.reason == MaskLossReason::Bitmap).count(),
        1,
        "a LEGACY bitmap zone — a recipe.json saved before the zones became \
             Select Sky components — stays engine-only"
    );
    assert_eq!(
        losses.iter().filter(|l| l.reason == MaskLossReason::RasterNotEmbedded).count(),
        1,
        "a raster that does not exist cannot ride in the payload: {losses:?}"
    );
    assert_eq!(losses.len(), 2);

    // The PROJECTION (payload-free): the mask is not in the crs settings.
    let projected = xmp_to_recipe(&bare_document(&recipe, None));
    assert_eq!(projected.exposure_ev, -0.8);
    assert_eq!(projected.temperature_k, Some(9000.0));
    assert_eq!(projected.tint, 10.0);
    assert_eq!(projected.saturation, 30.0);
    assert_eq!(projected.tone_curve, recipe.tone_curve);
    assert!(projected.masks.is_empty());
    // The whole document brings the mask back, by name, raster or not.
    assert_eq!(xmp_to_recipe(&doc).masks.len(), 1);
    assert!(projected.red_curve.is_empty());
    assert!(projected.green_curve.is_empty());
    assert!(projected.blue_curve.is_empty());
}

/// R24-5 M0, EXPORT direction: the sidecar cannot carry the camera base
/// curve or the lens-profile correction, and the user has to hear it —
/// silence there is a photo that renders differently in Lightroom for a
/// reason nothing on screen names.
///
/// Derived from the tier registry, so this test also pins the derivation:
/// a neutral recipe discloses nothing, and the disclosed names are exactly
/// the `RenderedNotExported` rows that are actually set.
#[test]
fn the_export_names_the_globals_the_sidecar_cannot_carry() {
    use crate::advisor::catalogue::{Tier, RECIPE_CONTROLS};
    assert!(
        global_export_losses(&EditRecipe::default()).is_empty(),
        "a neutral recipe loses nothing — a save that lost nothing must not interrupt"
    );
    // The engine's own measurement of THIS photo: rendered, unexportable.
    let with_base = EditRecipe {
        base_curve: vec![[0.0, 0.0], [0.5, 0.55], [1.0, 1.0]],
        ..Default::default()
    };
    assert_eq!(global_export_losses(&with_base), vec!["base_curve"]);
    let both = EditRecipe {
        lens_profile: crate::recipe::LensProfile {
            vignette_on: true,
            vignette: vec![1.0, 0.1],
            ..Default::default()
        },
        ..with_base.clone()
    };
    assert_eq!(global_export_losses(&both), vec!["base_curve", "lens_profile"]);
    // An ordinary rendered control is NOT a loss (it has its own crs key).
    let exposed = EditRecipe { exposure_ev: 1.5, ..Default::default() };
    assert!(global_export_losses(&exposed).is_empty());
    // Premise: the tier this is derived from is populated. A registry
    // where nobody is RenderedNotExported would make every case above
    // pass for the wrong reason.
    assert_eq!(
        RECIPE_CONTROLS
            .iter()
            .filter(|c| c.tier == Some(Tier::RenderedNotExported))
            .map(|c| c.name)
            .collect::<Vec<_>>(),
        // R33 §G added the third, and the first that is an EDIT rather
        // than the engine's own measurement of the photo; v1.5.0 F6 added
        // `upright_transform`, which is ADOBE'S measurement of it — read
        // from `crs:UprightTransform_N`, rendered, and never written back,
        // because writing it would claim Adobe's key for our own solver's
        // numbers. Registry order, not alphabetical.
        // In registry order, which is the panel's draw order: F7's creative
    // Look sits between the Upright matrices and the base curve because
    // that is where its field is declared.
    // v1.5.0 F9 adds `retouch`, and it is the one row here whose export
    // story has two halves. A MERGE keeps the photographer's own
    // `crs:RetouchAreas` verbatim (this writer does not own the element,
    // so it never strips it) — but a FRESH `recipe_to_xmp` emits nothing
    // for it, and on that path every removal is gone. The tier names the
    // worse half, which is what a disclosure is for.
    vec!["upright_transform", "look", "base_curve", "lens_profile", "retouch", "colour_field"],
    );
}

/// R24-5 M0, IMPORT direction: a Lightroom sidecar's globals that AutoShade
/// does not model. The merge keeps them; this is the sentence that says
/// they are there — the global counterpart of the mask-side
/// `unsupported_corrections`, which had no partner until now.
#[test]
fn an_imported_sidecar_names_the_globals_the_engine_does_not_render() {
    // FIXTURE NOTE, FOURTH REVISION. `Texture` / `GrainAmount` were the
    // samples until R25 B2 modelled them; `PerspectiveUpright` took over
    // and B4 has now claimed that too. `PointColor` served until v1.5.0
    // modelled Lightroom's point colours (the `crs:PointColors` element).
    // The samples are `CurveRefineSaturation` — the point curve's Refine
    // Saturation, which no AutoShade control holds — and
    // `CameraProfileDigest`, chosen deliberately: it sits one line from
    // `crs:CameraProfile` in every real sidecar, and B4 owns the profile
    // NAME while the digest stays foreign. The list shrinking under the
    // fixtures, batch after batch, IS the complement definition working.
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                   xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
                   <rdf:Description rdf:about=\"\" \
                   xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
                   crs:Exposure2012=\"+1.00\" crs:Texture=\"+30\" \
                   crs:CurveRefineSaturation=\"100\" crs:PerspectiveUpright=\"1\" \
                   crs:CameraProfile=\"Adobe Standard\" \
                   crs:CameraProfileDigest=\"2D1D4700365C3E2831EEAE0D1A8F9CDF\" \
                   crs:RawFileName=\"crs:NotAnAttribute=1.ARW\"/></rdf:RDF></x:xmpmeta>";
    let found = unmodelled_global_crs(doc);
    assert!(found.contains(&"CurveRefineSaturation".to_string()), "{found:?}");
    assert!(found.contains(&"CameraProfileDigest".to_string()), "{found:?}");
    // …and the B4 half of the same claim: the Transform block and the
    // profile NAME are ours now, so they left this list with no edit to it.
    assert!(
        !found.contains(&"PerspectiveUpright".to_string()),
        "PerspectiveUpright is passed through since R25 B4: {found:?}"
    );
    assert!(
        !found.contains(&"CameraProfile".to_string()),
        "the profile NAME is passed through; only its digest is foreign: {found:?}"
    );
    // A control we DO model is not "unmodelled" — the universe is the
    // complement of `owned_attr_keys`, which is what stops this list from
    // needing a catalogue of Adobe property names to keep up to date.
    assert!(!found.contains(&"Exposure2012".to_string()), "{found:?}");
    // …and the B2 half of that claim, which is the whole reason the
    // fixture above had to change: teaching the engine `crs:Texture` took
    // the key OFF this list with no edit to the list itself.
    assert!(
        !found.contains(&"Texture".to_string()),
        "Texture is modelled since R25 B2 and must have left this list: {found:?}"
    );
    // Quote-aware: `crs:` inside an attribute VALUE is text, not a
    // property (a RawFileName or a mask name may contain anything).
    assert!(!found.contains(&"NotAnAttribute".to_string()), "{found:?}");

    // Mask corrections live in a CHILD element and have their own
    // disclosure; reporting every crs:Local* key as an unmodelled global
    // would bury the real ones under sixty names.
    let r = EditRecipe {
        masks: vec![crate::recipe::LocalAdjustment {
            mask: crate::recipe::MaskGeometry::Linear {
                zero_x: 0.0,
                zero_y: 0.0,
                full_x: 0.0,
                full_y: 1.0,
            },
            enabled: true,
            amount: 1.0,
            exposure_ev: 0.5,
            ..Default::default()
        }],
        // Element-form children of OUR OWN making: the four tone curves
        // have no attribute spelling, so `owned_attr_keys` cannot exclude
        // them and only `OWNED_ELEMENT_ONLY` keeps the element arm below
        // from naming our own curves as Lightroom-only properties.
        tone_curve: vec![
            crate::recipe::CurvePoint { input: 0, output: 0 },
            crate::recipe::CurvePoint { input: 128, output: 140 },
            crate::recipe::CurvePoint { input: 255, output: 255 },
        ],
        red_curve: vec![
            crate::recipe::CurvePoint { input: 0, output: 0 },
            crate::recipe::CurvePoint { input: 255, output: 250 },
        ],
        ..Default::default()
    };
    let ours = recipe_to_xmp(&r);
    let found = unmodelled_global_crs(&ours);
    assert!(
        found.is_empty(),
        "a sidecar WE wrote models everything in it by construction: {found:?}"
    );
    // Nothing to read ⇒ nothing to say (never a panic, never a warning).
    assert!(unmodelled_global_crs("").is_empty());
    assert!(unmodelled_global_crs("<not xml").is_empty());
}

/// R24 round-end MED-1: the same disclosure for the PROPERTY-ELEMENT
/// spelling. `crs_str` reads that form "for exactly that reason" and the
/// merge strips it, because Lightroom writes it in plenty of real
/// sidecars — but this scanner walked the Description's open TAG only, so
/// an element-form catalog export disclosed nothing at all and the photo
/// just rendered differently from Lightroom with no sentence on screen.
#[test]
fn the_import_disclosure_reads_property_element_globals_too() {
    let head = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                    xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">";
    let tail = "</rdf:RDF></x:xmpmeta>";
    // (a) Pure element form — the shape that returned EMPTY before.
    // (Same fixture note as the test above, third revision: `Texture` is
    // modelled since B2 and `PerspectiveUpright` is passed through since
    // B4, so the unmodelled samples are `PointColor` and
    // `UprightTransform` — the Upright SOLVER's own opaque blob, which is
    // in six of the seven reference sidecars and stays foreign.)
    let element = format!(
        "{head}<rdf:Description rdf:about=\"\" \
             xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\">\
             <crs:Exposure2012>+1.00</crs:Exposure2012>\
             <crs:Texture>+30</crs:Texture>\
             <crs:PerspectiveUpright>1</crs:PerspectiveUpright>\
             <crs:PointColor>0</crs:PointColor>\
             <crs:UprightTransform>1.0000000</crs:UprightTransform>\
             </rdf:Description>{tail}"
    );
    let found = unmodelled_global_crs(&element);
    assert_eq!(
        found,
        vec!["PointColor", "UprightTransform"],
        "element-form globals must be named exactly once, and never the modelled \
             Exposure2012 / Texture or the passed-through PerspectiveUpright"
    );

    // (b) MIXED: Lightroom splits the same Description across both forms.
    let mixed = format!(
        "{head}<rdf:Description rdf:about=\"\" \
             xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
             crs:Exposure2012=\"+1.00\" crs:PointColor=\"0\">\
             <crs:UprightTransform>1.0000000</crs:UprightTransform>\
             </rdf:Description>{tail}"
    );
    assert_eq!(unmodelled_global_crs(&mixed), vec!["PointColor", "UprightTransform"]);

    // (c) The two exclusions the element walk must keep: a mask block's
    // `crs:Local*` items (their own disclosure) and a creative Look's
    // baked parameters (someone else's settings block, nested inside a
    // child) are NOT this Description's globals. `Look` itself IS one.
    let nested = format!(
        "{head}<rdf:Description rdf:about=\"\" \
             xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\">\
             <crs:PointColor>0</crs:PointColor>\
             <crs:Look><rdf:Description><crs:Parameters><rdf:Description>\
             <crs:LookClarity2012>+50</crs:LookClarity2012>\
             </rdf:Description></crs:Parameters></rdf:Description></crs:Look>\
             <crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li>\
             <rdf:Description crs:LocalExposure2012=\"0.5\" crs:LocalTexture=\"20\"/>\
             </rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections>\
             </rdf:Description>{tail}"
    );
    assert_eq!(unmodelled_global_crs(&nested), vec!["Look", "PointColor"]);

    // (d) NIT-1: `-` and `.` are legal XML name characters. No Adobe key
    // uses either today, so this pins the reading rather than a change:
    // the whole name is reported, not the prefix before the hyphen.
    assert_eq!(
        unmodelled_global_crs(
            "<rdf:Description xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
                 crs:Foo-Bar=\"1\" crs:Plain=\"2\"/>"
        ),
        vec!["Foo-Bar", "Plain"]
    );
}

/// `Eq ::= S? '=' S?` — XML lets any whitespace (space, tab, CR, LF) sit
/// between an attribute's name and its `=`, and the attribute reader
/// (`next_xml_attribute`) accepts all four. This scan skipped ASCII spaces
/// only, so a foreign key wrapped as `crs:Foo\n="1"` was kept by the merge
/// and missing from the disclosure that says it is there.
///
/// MUTATION THIS CATCHES: `is_ascii_whitespace()` back to `== ' '` and
/// both keys vanish from the list.
#[test]
fn an_unmodelled_key_is_named_whatever_xml_whitespace_precedes_its_equals_sign() {
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                   xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
                   <rdf:Description rdf:about=\"\" \
                   xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
                   crs:Exposure2012 = \"+1.00\" \
                   crs:CurveRefineSaturation\t=\t\"100\"\n\
                   crs:CameraProfileDigest\r\n=\n\"2D1D4700365C3E2831EEAE0D1A8F9CDF\"/>\
                   </rdf:RDF></x:xmpmeta>";
    assert_eq!(
        unmodelled_global_crs(doc),
        vec!["CameraProfileDigest", "CurveRefineSaturation"],
        "tab, CR and LF before `=` spell the same attribute as a space"
    );
}

/// L03-3: the import gate must MATCH the disclosure sentence — a
/// conflicting crs binding imports nothing, because the scanners would
/// read properties through a prefix the document bound elsewhere.
#[test]
fn a_conflicting_crs_binding_imports_nothing() {
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                   xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
                   <rdf:Description rdf:about=\"\" xmlns:crs=\"urn:other\" \
                   crs:Exposure2012=\"+2.50\"/></rdf:RDF></x:xmpmeta>";
    assert!(xmlns_conflict(doc).is_some(), "the binding is a conflict");
    let r = xmp_to_recipe(doc);
    assert_eq!(
        r.exposure_ev, 0.0,
        "settings under a conflicting binding must not import — the disclosure says they were not"
    );
    assert!(
        unparsable_crs_numbers(doc)[0].contains("not imported"),
        "and the disclosure names the refusal"
    );
}

/// L03-3's mask half: the import refuses the WHOLE document under a
/// conflicting binding, and the disclosure sentence says its settings
/// were not imported — so the per-correction loss list, the drop count
/// and the carried-globals list have nothing to add. Each used to be read
/// through the very prefix the gate had just declared unreliable, and
/// reported masks skipped for reasons that were never the reason.
///
/// MUTATION THIS CATCHES: drop `xmlns_conflict` from any one of the three
/// gates and its assertion on the conflicting document fails.
#[test]
fn a_conflicting_crs_binding_discloses_no_mask_or_global_losses() {
    let muted = lr_radial("0", "0").replace("crs:MaskValue=\"1\"", "crs:MaskValue=\"0\"");
    let doc = lr_doc(&lr_correction("Radial 1", "", &muted)).replace(
        "crs:Exposure2012=\"+0.35\"",
        "crs:Exposure2012=\"+0.35\"\n   crs:CurveRefineSaturation=\"100\"",
    );
    // Premise: under the canonical binding all three channels speak.
    assert_eq!(unsupported_corrections(&doc), 1);
    assert!(!import_losses(&doc).is_empty());
    assert!(
        unmodelled_global_crs(&doc).contains(&"CurveRefineSaturation".to_string()),
        "{:?}",
        unmodelled_global_crs(&doc)
    );
    let conflict = doc.replace(CRS_URI, "urn:other");
    assert!(xmlns_conflict(&conflict).is_some(), "premise: the binding is a conflict");
    assert!(xmp_to_recipe(&conflict).masks.is_empty(), "premise: nothing imports");
    assert_eq!(unsupported_corrections(&conflict), 0, "no drop count on a refused document");
    assert!(import_losses(&conflict).is_empty(), "{:?}", import_losses(&conflict));
    assert!(
        unmodelled_global_crs(&conflict).is_empty(),
        "{:?}",
        unmodelled_global_crs(&conflict)
    );
    // …and the photo-aware doors share the gate (it answers before any
    // sibling table is looked for, so the path need not exist).
    let photo = std::path::Path::new("synthetic.arw");
    assert_eq!(unsupported_corrections_for_photo(&conflict, photo), 0);
    assert!(import_losses_for_photo(&conflict, photo).is_empty());
}

/// L03-4: the DEFAULT namespace declaration (bare `xmlns=`) bound to the
/// camera-raw or RDF namespace is the same conflict as a foreign prefix —
/// it hides settings in unprefixed spellings the scanners cannot see.
#[test]
fn a_default_namespace_binding_to_crs_is_a_conflict() {
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                   xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
                   <rdf:Description rdf:about=\"\" \
                   xmlns=\"http://ns.adobe.com/camera-raw-settings/1.0/\">\
                   <Exposure2012>+1.00</Exposure2012>\
                   </rdf:Description></rdf:RDF></x:xmpmeta>";
    let why = xmlns_conflict(doc).expect("a default-namespace binding to crs must refuse");
    assert!(why.contains("DEFAULT namespace"), "the reason names the binding: {why}");
    assert_eq!(xmp_to_recipe(doc).exposure_ev, 0.0);
}

/// R12-03: bindings resolve in SCOPE — a nested island that rebinds `crs`
/// around content that never says `crs:` is somebody else's metadata, not
/// a reason to throw away the whole document's settings.
#[test]
fn an_unused_nested_rebind_no_longer_refuses_the_document() {
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                   xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
                   <rdf:Description rdf:about=\"\" \
                   xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
                   crs:Exposure2012=\"+0.50\">\
                   <dc:island xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
                   xmlns:crs=\"urn:other\"><dc:note>hi</dc:note></dc:island>\
                   </rdf:Description></rdf:RDF></x:xmpmeta>";
    assert!(xmlns_conflict(doc).is_none(), "an unused rebind is harmless");
    assert_eq!(xmp_to_recipe(doc).exposure_ev, 0.5, "and the settings import");
}

/// R12-03: the rebind still refuses wherever a `crs:` name actually
/// RESOLVES through it — here on a descendant deep inside the island.
#[test]
fn a_rebind_refuses_exactly_where_a_name_resolves_through_it() {
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                   xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
                   <dc:island xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
                   xmlns:crs=\"urn:other\">\
                   <dc:inner crs:Shadows2012=\"+10\"/></dc:island>\
                   </rdf:RDF></x:xmpmeta>";
    let why = xmlns_conflict(doc).expect("a name resolving through the rebind refuses");
    assert!(why.contains("urn:other"), "the reason names the binding: {why}");
}

/// R12-03: a foreign alias for the camera-raw URI is inert while no name
/// resolves through it, and a conflict the moment one does — settings
/// spelled through the alias are invisible to the `crs:` scanners.
#[test]
fn a_foreign_alias_for_the_crs_uri_refuses_only_when_used() {
    let head = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                    xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\" \
                    xmlns:zzz=\"http://ns.adobe.com/camera-raw-settings/1.0/\">\
                    <rdf:Description rdf:about=\"\"";
    let unused = format!("{head}/></rdf:RDF></x:xmpmeta>");
    assert!(xmlns_conflict(&unused).is_none(), "declared but never used");
    let used = format!("{head} zzz:Exposure2012=\"+1.00\"/></rdf:RDF></x:xmpmeta>");
    let why = xmlns_conflict(&used).expect("a name through the alias refuses");
    assert!(why.contains("`zzz:`"), "the reason names the prefix: {why}");
}

/// R12-03: a scope ends at its element's close tag — the island's rebind
/// must not leak forward onto a following sibling whose `crs:` names
/// resolve through the document-level canonical binding.
#[test]
fn a_closed_scope_releases_its_binding() {
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                   xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\" \
                   xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\">\
                   <dc:island xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
                   xmlns:crs=\"urn:other\"><dc:note>hi</dc:note></dc:island>\
                   <rdf:Description rdf:about=\"\" crs:Exposure2012=\"+0.50\"/>\
                   </rdf:RDF></x:xmpmeta>";
    assert!(
        xmlns_conflict(doc).is_none(),
        "the sibling's crs resolves through the canonical ancestor binding"
    );
}

/// R12-03 coordination: the scoped gate now clears a Description whose
/// foreign `xmlns:crs` is unused — so the merge's target finder must not
/// key on the attribute NAME alone, or it would splice canonical-intent
/// `crs:` settings into a scope where `crs` means something else.
#[test]
fn the_merge_skips_a_description_whose_crs_binding_is_foreign() {
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                   xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
                   <rdf:Description rdf:about=\"\" xmlns:crs=\"urn:other\"/>\
                   </rdf:RDF></x:xmpmeta>";
    assert!(xmlns_conflict(doc).is_none(), "unused foreign binding is cleared");
    assert_eq!(
        find_crs_description(doc),
        None,
        "and the merge must not adopt that Description as its settings target"
    );
}

/// R12-03: past the scope-tracking bound the gate cannot prove a binding
/// harmless, so it refuses conservatively — never silently accepts.
#[test]
fn deeper_xmlns_nesting_than_tracked_refuses_conservatively() {
    let mut doc = String::new();
    for _ in 0..1025 {
        doc.push_str("<t xmlns:q=\"urn:x\">");
    }
    let why = xmlns_conflict(&doc).expect("beyond the bound is a refusal");
    assert!(why.contains("more xmlns declarations"), "{why}");
}

/// R13-01 (round-13 Codex review): a SURPLUS or MISNAMED close tag must
/// not release a foreign binding early — pops are paired by name, not by
/// arithmetic alone, so malformed nesting degrades toward refusal.
#[test]
fn a_mismatched_close_does_not_release_a_foreign_binding() {
    let doc = "<rdf:Description \
                   xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\" \
                   xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\">\
                   <island xmlns:crs=\"urn:foreign\"></bogus>\
                   <crs:Exposure2012>+2.0</crs:Exposure2012>\
                   </island></rdf:Description>";
    let why = xmlns_conflict(doc)
        .expect("the crs name still resolves through the un-closed island's rebind");
    assert!(why.contains("urn:foreign"), "the reason names the live binding: {why}");
}

/// R13-02 (round-13 Codex review): the tracking bound counts LIVE
/// DECLARATIONS, not frames — a single tag carrying a declaration flood
/// is past what the gate can resolve affordably, so it refuses.
#[test]
fn a_flat_declaration_flood_refuses_conservatively() {
    let mut doc = String::from("<t");
    for i in 0..257 {
        doc.push_str(&format!(" xmlns:q{i}=\"urn:x\""));
    }
    doc.push('>');
    let why = xmlns_conflict(&doc).expect("a declaration flood is a refusal");
    assert!(why.contains("more xmlns declarations"), "{why}");
}

/// L03-7: curve items are matched by tag name — a whitespace-spelled
/// `<rdf:li >` is a real item, not an invisible one that empties the
/// curve (and lets the next save delete it).
#[test]
fn a_whitespace_spelled_curve_item_is_still_a_curve_point() {
    let scope = "<crs:ToneCurvePV2012><rdf:Seq>\
                     <rdf:li >128, 64</rdf:li >\
                     <rdf:li>255, 255</rdf:li>\
                     </rdf:Seq></crs:ToneCurvePV2012>";
    assert_eq!(
        parse_curve_checked(scope, "ToneCurvePV2012"),
        Ok(vec![
            CurvePoint { input: 128, output: 64 },
            CurvePoint { input: 255, output: 255 },
        ]),
        "both spellings are legal XML for the same element"
    );
}

/// L03-9: HasCrop="True" whose coordinates are missing or inverted still
/// imports as no-crop (clamping half a geometry would change coverage),
/// but the drop is DISCLOSED — the next save persists HasCrop="False",
/// and silence made that a deletion nobody asked for.
#[test]
fn an_inconsistent_crop_is_disclosed_not_silently_dropped() {
    let head = "<rdf:Description rdf:about=\"\" \
                    xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
                    crs:HasCrop=\"True\" crs:CropLeft=\"0.1\" crs:CropTop=\"0.1\" \
                    crs:CropRight=\"0.9\"/>";
    assert!(xmp_to_recipe(head).crop.is_none(), "a missing coordinate cannot crop");
    assert!(
        unparsable_crs_numbers(head).iter().any(|k| k.starts_with("Crop")),
        "the missing coordinate is disclosed: {:?}",
        unparsable_crs_numbers(head)
    );

    let inverted = "<rdf:Description rdf:about=\"\" \
                        xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
                        crs:HasCrop=\"True\" crs:CropLeft=\"0.8\" crs:CropTop=\"0.1\" \
                        crs:CropRight=\"0.2\" crs:CropBottom=\"0.9\"/>";
    assert!(xmp_to_recipe(inverted).crop.is_none());
    assert!(
        unparsable_crs_numbers(inverted).iter().any(|k| k.starts_with("Crop")),
        "inverted ordering is disclosed"
    );

    let fine = "<rdf:Description rdf:about=\"\" \
                    xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
                    crs:HasCrop=\"True\" crs:CropLeft=\"0.1\" crs:CropTop=\"0.1\" \
                    crs:CropRight=\"0.9\" crs:CropBottom=\"0.9\"/>";
    assert!(xmp_to_recipe(fine).crop.is_some());
    assert!(
        unparsable_crs_numbers(fine).is_empty(),
        "a consistent crop discloses nothing"
    );
}

/// `Left > Right` under a non-zero `CropAngle` is a legal Lightroom
/// arrangement (`P3-cropangle-model.md` §6.3;
/// `an_inverted_crop_arrangement_is_read_rather_than_discarded` covers
/// the decoder), and the reader imports it whole. The disclosure restated
/// the reader's ordering rule as `Left < Right && Top < Bottom`
/// unconditionally, so it called this crop "inconsistent" while the
/// recipe carried it — two faces of one document disagreeing.
///
/// MUTATION THIS CATCHES: put the ordering predicate back in place of the
/// `read_crop` verdict and the rotated arrangement is disclosed again.
#[test]
fn a_rotated_left_over_right_crop_the_reader_accepts_is_not_disclosed() {
    let frame = FrameAspect::from_size(9504.0, 6336.0);
    let engine = Crop { left: 0.30, top: 0.10, right: 0.62, bottom: 0.90 };
    let lr = engine_to_lr_crop(Some(&engine), -35.0, frame).expect("corners");
    assert!(lr.left > lr.right, "the fixture must reach the inverted region: {lr:?}");
    let angle = lr_num(lr.angle_deg);
    let doc = in_frame(&lr_doc(""), 9504, 6336).replace(
        "crs:Version=\"15.5.1\"",
        &format!(
            "crs:Version=\"15.5.1\"\n   crs:HasCrop=\"True\"\n   crs:CropLeft=\"{}\"\n   \
                 crs:CropTop=\"{}\"\n   crs:CropRight=\"{}\"\n   crs:CropBottom=\"{}\"\n   \
                 crs:CropAngle=\"{angle}\"",
            lr_num(lr.left),
            lr_num(lr.top),
            lr_num(lr.right),
            lr_num(lr.bottom),
        ),
    );
    let r = xmp_to_recipe(&doc);
    assert!(r.crop.is_some(), "premise: the reader imports the rotated arrangement");
    assert!((r.straighten_deg + 35.0).abs() < 1e-4, "{}", r.straighten_deg);
    assert!(
        unparsable_crs_numbers(&doc).is_empty(),
        "a crop the reader imported whole is not \"inconsistent\": {:?}",
        unparsable_crs_numbers(&doc)
    );
    // …while the SAME corners at θ = 0 are the inverted rectangle they
    // look like: refused by the reader, and disclosed here — the verdict
    // follows the decoder, not the ordering.
    let flat = doc.replace(
        &format!("crs:CropAngle=\"{angle}\""),
        "crs:CropAngle=\"0\"",
    );
    assert!(xmp_to_recipe(&flat).crop.is_none(), "premise: refused at θ = 0");
    assert!(
        unparsable_crs_numbers(&flat).iter().any(|k| k.starts_with("Crop")),
        "{:?}",
        unparsable_crs_numbers(&flat)
    );
}

/// L03-18: raw tab/newline in an attribute value would be folded to
/// spaces by any compliant parser's attribute-value normalization —
/// character references survive it, and our reader decodes them back.
#[test]
fn attribute_control_characters_survive_as_character_references() {
    assert_eq!(xml_attr_escape("a\tb\nc\rd"), "a&#9;b&#10;c&#13;d");
    assert_eq!(xml_unescape("a&#9;b&#10;c&#13;d").as_ref(), "a\tb\nc\rd");
}

use crate::recipe::{CurvePoint, EditRecipe, LocalAdjustment};

/// The merged document alone — most tests assert on the text; the ones
/// about [`MergeOutcome::notes`] call the real function.
fn merged_doc(existing: &str, r: &EditRecipe) -> Option<String> {
    merge_recipe_into_xmp(existing, r).map(|o| o.doc)
}

/// The scope scanner meets a sidecar that is hostile rather than merely
/// unusual. Both halves were real defects: the close search restarted on
/// every nested open (Θ(k²) — this document took MINUTES before, inside
/// SAVE_LOCK and holding a server request permit), and a
/// `</rdf:Description>` inside a COMMENT was read as a real close, which
/// truncated the body and sank the whole merge to a fresh document,
/// dropping the Lightroom-only properties the merge exists to preserve.
#[test]
fn a_pathological_sidecar_neither_hangs_nor_believes_a_comment() {
    // (a) Deep nesting: linear now, quadratic before. 20 000 opens is
    // ~0.4 MB. MEASURED on this box: 0.02 s with the cached close cursor,
    // 13.62 s when the cache is removed — 680x, on a file a user could
    // receive by opening someone else's shoot. The assertion below pins
    // correctness; the wall clock is the pin on the complexity, so keep
    // the size when editing this test.
    let mut doc = String::from(
        r#"<rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="+0.50">"#,
    );
    let gt = doc.len() - 1;
    for _ in 0..20_000 {
        doc.push_str("<rdf:Description>");
    }
    for _ in 0..20_000 {
        doc.push_str("</rdf:Description>");
    }
    doc.push_str("</rdf:Description>");
    let close = find_matching_close(&doc, gt + 1).expect("the outermost close is found");
    assert_eq!(&doc[close..close + 18], "</rdf:Description>");
    assert_eq!(close, doc.len() - 18, "it is the LAST one, not an inner one");

    // (b) A comment holding the close literal is TEXT, not a close.
    let doc = format!(
        "{}<!-- </rdf:Description> --><crs:Texture>25</crs:Texture></rdf:Description>",
        r#"<rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">"#
    );
    let gt = doc.find('>').unwrap();
    let close = find_matching_close(&doc, gt + 1).expect("the real close is found");
    assert_eq!(close, doc.len() - 18, "the comment's copy is not a close");
    // …and the scope therefore still carries the child that follows it.
    let scope = crs_own_scope(&doc);
    assert!(scope.contains("crs:Texture"), "the body survived the comment: {scope}");

    // (c) CDATA gets the same treatment.
    let doc = format!(
        "{}<![CDATA[ </rdf:Description> ]]><crs:Texture>25</crs:Texture></rdf:Description>",
        r#"<rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">"#
    );
    let gt = doc.find('>').unwrap();
    assert_eq!(find_matching_close(&doc, gt + 1), Some(doc.len() - 18));

    // (d) An UNTERMINATED comment is unaccountable markup: no close at all,
    // so the caller falls back to the whole document rather than guessing.
    let doc = format!(
        "{}<!-- </rdf:Description>",
        r#"<rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">"#
    );
    let gt = doc.find('>').unwrap();
    assert_eq!(find_matching_close(&doc, gt + 1), None);
}

/// The complexity itself, asserted — because the correctness test above
/// passes at ANY speed, which is how the second blowup shipped.
///
/// The scanner has had two separate quadratic shapes. The cached close
/// cursor killed the first (nesting) and the construct skip it shipped
/// alongside introduced the second, on a shape half the size: measured
/// with release-mode replicas of the committed code, 640 KB of
/// back-to-back comments took **8.47 s** and 400 KB of PIs **9.59 s**,
/// against 51 µs and 90 µs for the code that predated the construct skip.
/// Quadratic scaling (4x bytes -> 16x time) put the 16 MiB `read_sidecar`
/// ceiling at roughly an hour and a half — spent inside SAVE_LOCK holding
/// one of the server's eight request permits, reachable by SELECTING a
/// photo that has such a sidecar beside it.
///
/// So both shapes are pinned by wall clock here. The budget is deliberately
/// loose (a debug build on a loaded CI box is not a benchmark); it only has
/// to separate "linear" from "quadratic", and the gap is five orders of
/// magnitude. Keep the SIZES if you edit this test — they are the pin.
#[test]
fn the_scope_scanner_is_linear_on_both_pathological_shapes() {
    const BUDGET: std::time::Duration = std::time::Duration::from_secs(10);
    let head = r#"<rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">"#;

    // Shape 1 — deep nesting (the first blowup). 80 000 opens, ~2.8 MB.
    let mut nested = String::from(head);
    let gt = nested.len() - 1;
    for _ in 0..80_000 {
        nested.push_str("<rdf:Description>");
    }
    for _ in 0..80_000 {
        nested.push_str("</rdf:Description>");
    }
    nested.push_str("</rdf:Description>");

    // Shape 2 — a body of back-to-back comments (the second blowup),
    // and 3 — the same with PIs, which took even longer per byte.
    let commented = format!("{head}{}</rdf:Description>", "<!--x-->".repeat(80_000));
    let pis = format!("{head}{}</rdf:Description>", "<?p?>".repeat(80_000));

    for (name, doc) in [("nested", &nested), ("comments", &commented), ("PIs", &pis)] {
        let started = std::time::Instant::now();
        let close = find_matching_close(doc, gt + 1).expect("the outermost close is found");
        let elapsed = started.elapsed();
        assert_eq!(close, doc.len() - 18, "{name}: it is the LAST close, not an inner one");
        assert!(
            elapsed < BUDGET,
            "{name}: {} bytes scanned in {elapsed:?}, over the {BUDGET:?} budget — \
                 a landmark cursor is being recomputed on every iteration again",
            doc.len()
        );
    }
}

/// A creative profile's baked parameters are the PROFILE's, never the
/// photographer's. Adobe nests them as owned-LOOKING crs children of a
/// second `rdf:Description` (`<crs:Look><rdf:Description><crs:Parameters>
/// <rdf:Description><crs:Clarity2012>…`) — the exact shape the WRITER's
/// depth-aware strip was built for. The reader's flat scan answered from
/// them whenever the top level omitted the key, so opening such a sidecar
/// wrote the profile's look into the user's sliders and the next save
/// persisted it.
#[test]
fn a_nested_look_is_not_a_user_edit() {
    let doc = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    crs:Version="15.5.1"
    crs:Exposure2012="+0.20">
   <crs:Look>
    <rdf:Description crs:Name="Adobe Landscape">
     <crs:Parameters>
      <rdf:Description>
       <crs:Clarity2012>+50</crs:Clarity2012>
       <crs:Vibrance>+35</crs:Vibrance>
       <crs:ToneCurvePV2012>
        <rdf:Seq>
         <rdf:li>0, 30</rdf:li>
         <rdf:li>255, 255</rdf:li>
        </rdf:Seq>
       </crs:ToneCurvePV2012>
      </rdf:Description>
     </crs:Parameters>
    </rdf:Description>
   </crs:Look>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
"#;
    let r = xmp_to_recipe(doc);
    assert_eq!(r.exposure_ev, 0.20, "the Description's OWN attribute imports");
    assert_eq!(r.clarity, 0.0, "the Look's Clarity2012 is not a user edit: {}", r.clarity);
    assert_eq!(r.vibrance, 0.0, "the Look's Vibrance is not a user edit: {}", r.vibrance);
    assert!(r.tone_curve.is_empty(), "the Look's baked curve is not a user curve");
    // The disclosure follows the import: a corrupt number the import never
    // reads must not be announced as a setting that will be lost.
    let corrupt = doc.replace("<crs:Clarity2012>+50</crs:Clarity2012>", "<crs:Clarity2012>--</crs:Clarity2012>");
    assert!(
        unparsable_crs_numbers(&corrupt).is_empty(),
        "only settings the import READS may be disclosed: {:?}",
        unparsable_crs_numbers(&corrupt)
    );
    // …and the same key AT top level still imports, in both spellings.
    for own in [
        r#"crs:Exposure2012="+0.20" crs:Clarity2012="+12""#.to_string(),
        r#"crs:Exposure2012="+0.20">
   <crs:Clarity2012>+12</crs:Clarity2012"#
            .to_string(),
    ] {
        let d = doc.replace(r#"crs:Exposure2012="+0.20""#, &own);
        assert_eq!(xmp_to_recipe(&d).clarity, 12.0, "own Clarity2012 must import: {own}");
    }
}

/// The scope keeps what the Description really owns — masks (whose nested
/// Descriptions are its own mask items) and plain property elements — and
/// falls back to the whole document when the markup cannot be accounted
/// for, which is the pre-scope behaviour.
#[test]
fn the_crs_scope_keeps_owned_children_and_degrades_safely() {
    let mut r = EditRecipe {
        exposure_ev: 0.5,
        tone_curve: vec![CurvePoint { input: 0, output: 12 }, CurvePoint { input: 255, output: 250 }],
        ..Default::default()
    };
    r.masks.push(LocalAdjustment {
        mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.9, full_x: 0.5, full_y: 0.1 },
        name: "sky".into(),
        exposure_ev: -0.4,
        ..Default::default()
    });
    // A full round-trip through the scope: masks and curves are OWNED and
    // must survive it.
    let doc = recipe_to_xmp(&r);
    let back = xmp_to_recipe(&doc);
    assert_eq!(back.tone_curve, r.tone_curve, "the owned tone curve survives the scope");
    assert_eq!(back.masks.len(), 1, "owned mask corrections survive the scope");
    assert_eq!(back.masks[0].name, "sky");
    // Markup the scanner cannot account for (an unclosed element) falls
    // back to the whole document rather than losing every setting.
    let broken = doc.replace("</rdf:Description>", "");
    assert!(crs_scope_inner(&broken).is_none(), "unaccountable markup yields no scope");
    assert_eq!(
        xmp_to_recipe(&broken).exposure_ev,
        0.5,
        "the fallback still reads the document"
    );
}

#[test]
fn straighten_only_activates_crop_and_round_trips_to_no_crop() {
    // Lightroom applies CropAngle only under HasCrop="True" — a
    // straighten-only recipe ships the full frame as its carrier, and the
    // reader collapses that full-frame rectangle back to None.
    let r = EditRecipe { straighten_deg: 2.5, ..Default::default() };
    let x = recipe_to_xmp(&r);
    assert!(x.contains("crs:HasCrop=\"True\""), "straighten must activate the crop state");
    // R27: `crs:CropAngle` is the NEGATION of this engine's clockwise
    // straighten (`P3-cropangle-model.md` §4 — Lightroom turns the content
    // counter-clockwise by +CropAngle, measured on six photographs, 34×
    // margin on the weakest), and it goes out with Lightroom's own six
    // decimals rather than the one this writer used to round to (§6.4).
    assert!(x.contains("crs:CropAngle=\"-2.500000\""), "{x}");
    let back = xmp_to_recipe(&x);
    assert_eq!(back.crop, None, "the full-frame carrier must not become a real crop");
    assert_eq!(back.straighten_deg, 2.5);
    // Control chars in a mask name must not poison the document.
    let dirty = EditRecipe {
        masks: vec![LocalAdjustment { name: "sky\u{0}\u{7}".into(), ..Default::default() }],
        ..Default::default()
    };
    let x = recipe_to_xmp(&dirty);
    assert!(!x.contains('\u{0}') && !x.contains('\u{7}'), "forbidden chars stripped");
}

#[test]
fn renders_local_masks_with_correct_scale() {
    let r = EditRecipe {
        masks: vec![
            LocalAdjustment {
                mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.35, full_x: 0.5, full_y: 0.0 },
                name: "sky".into(),
                exposure_ev: -0.4,  // ÷4 → -0.1
                highlights: -50.0,  // ÷100 → -0.5
                ..Default::default()
            },
            LocalAdjustment {
                mask: MaskGeometry::Radial {
                    top: 0.3, left: 0.35, bottom: 0.7, right: 0.65,
                    feather: 0.5, roundness: 0.0, flipped: false, angle: 0.0,
                    midpoint: 50.0, mask_version: 2,
                },
                name: "subject".into(),
                shadows: 20.0,      // ÷100 → 0.2
                inverted: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let xmp = recipe_to_xmp(&r);
    // Write it out so well-formedness can be validated by an XML parser
    // while debugging — under the temp dir, never the working directory:
    // leaving `out/_masks_test.xmp` behind on every `cargo test` was a
    // side effect on the tree. Verification aid, not a behavioural
    // assertion; removed again at the end.
    let dir = std::env::temp_dir()
        .join(format!("autoshade-masks-xml-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok();
    std::fs::write(dir.join("_masks_test.xmp"), &xmp).ok();
    assert!(xmp.contains("<crs:MaskGroupBasedCorrections>"));
    assert!(xmp.contains(r#"crs:What="Mask/Gradient""#));
    assert!(xmp.contains(r#"crs:What="Mask/CircularGradient""#));
    // local scale conversions
    assert!(xmp.contains(r#"crs:LocalExposure2012="-0.1""#)); // -0.4 / 4
    assert!(xmp.contains(r#"crs:LocalHighlights2012="-0.5""#)); // -50 / 100
    assert!(xmp.contains(r#"crs:LocalShadows2012="0.2""#)); // 20 / 100
    assert!(xmp.contains(r#"crs:MaskInverted="true""#));
    assert!(xmp.contains(r#"crs:ZeroX="0.5""#));
    // Feather crosses the boundary on Lightroom's 0..100 scale (engine 0.5
    // → crs 50) — the old writer's raw "0.5" read in LR as a hard edge.
    assert!(xmp.contains(r#"crs:Feather="50""#));
    // unset masks ⇒ no mask block (v1-compatible)
    assert!(!recipe_to_xmp(&EditRecipe::default()).contains("MaskGroupBasedCorrections"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn radial_feather_converts_both_ways_and_keeps_legacy_own_scale() {
    // LR-style integer feather imports onto the engine's 0..1 scale…
    //
    // R25 P9: wrapped in a real `<rdf:li …/>`. It used to be a bare
    // attribute run, which worked only because the geometry was located by
    // substring; `base_geometry_at` scans TAGS, because choosing the base
    // out of several components means reading each component's own
    // `crs:MaskBlendMode` and that is a per-tag question. Every production
    // caller already hands whole markup — `classify_correction`, the only
    // one, tag-scans the same block itself — so the fixture was the thing
    // that did not look like a sidecar.
    //
    // R27 Batch-4 took the same step again, one level out: the component
    // list is now located by NAME (`crs:CorrectionMasks`) rather than
    // scanned for anywhere in the segment, because "which components are
    // this correction's own" is the question hazards 1 and 2 turned on.
    // `classify_correction` has always required that element — it returns
    // `Unrepresentable` without one — so the two now agree about where a
    // correction's components live, and the fixture wears the wrapper its
    // production caller always supplies.
    let li = r#"<crs:CorrectionMasks><rdf:Seq><rdf:li crs:What="Mask/CircularGradient" crs:Top="0.2" crs:Left="0.2" crs:Bottom="0.8" crs:Right="0.8" crs:Feather="72" crs:Roundness="0" crs:Flipped="false"/></rdf:Seq></crs:CorrectionMasks>"#;
    // The correction's own scope is empty here BY CONSTRUCTION: this
    // fixture is a bare component list with no correction tag around it,
    // so there are no sliders to read and the geometry is the whole claim.
    let m = parse_one_correction(li, Scope::new(""), None).expect("radial parses");
    let MaskGeometry::Radial { feather, .. } = m.mask else { panic!("radial") };
    assert!((feather - 0.72).abs() < 1e-6, "LR 72 → 0.72, got {feather}");
    // …while a legacy own-writer value (≤ 1.0) passes through verbatim.
    let legacy = li.replace(r#"crs:Feather="72""#, r#"crs:Feather="0.4""#);
    let m =
        parse_one_correction(&legacy, Scope::new(""), None).expect("legacy radial parses");
    let MaskGeometry::Radial { feather, .. } = m.mask else { panic!("radial") };
    assert!((feather - 0.4).abs() < 1e-6, "legacy 0.4 stays 0.4, got {feather}");
}

#[test]
fn renders_manual_vignette_only_when_set() {
    let r = EditRecipe { lens_vignette: 35.0, lens_vignette_mid: 60.0, ..Default::default() };
    let xmp = recipe_to_xmp(&r);
    assert!(xmp.contains(r#"crs:VignetteAmount="+35""#));
    assert!(xmp.contains(r#"crs:VignetteMidpoint="60""#));
    // A neutral recipe emits neither key (byte-compatible with the old writer).
    let neutral = recipe_to_xmp(&EditRecipe::default());
    assert!(!neutral.contains("VignetteAmount") && !neutral.contains("VignetteMidpoint"));
}

#[test]
fn renders_manual_distortion_only_when_set() {
    let r = EditRecipe { lens_distortion: -24.0, ..Default::default() };
    assert!(recipe_to_xmp(&r).contains(r#"crs:LensManualDistortionAmount="-24""#));
    let pos = EditRecipe { lens_distortion: 80.0, ..Default::default() };
    assert!(recipe_to_xmp(&pos).contains(r#"crs:LensManualDistortionAmount="+80""#));
    // Zero amount emits no key at all (byte-compatible with the old writer).
    assert!(!recipe_to_xmp(&EditRecipe::default()).contains("LensManualDistortionAmount"));
}

/// R25 B2: global Texture round-trips through its own `crs:Texture` key,
/// in Lightroom's own signed form.
///
/// READ AND WRITE IN ONE BATCH is not a nicety here. `owned_attr_keys` is
/// the WRITER's universe and also the merge's STRIP universe, so a
/// read-only Texture would have left Lightroom's value in the document
/// beside ours (two answers for one slider), and a write-only one would
/// have gone on being named by `unmodelled_global_crs` while quietly
/// rendering. Either half alone is a defect; this pins both.
#[test]
fn texture_round_trips_through_xmp() {
    let xmp = recipe_to_xmp(&EditRecipe { texture: 26.0, ..Default::default() });
    assert!(xmp.contains(r#"crs:Texture="+26""#), "the signed form Lightroom writes: {xmp}");
    assert_eq!(xmp_to_recipe(&xmp).texture, 26.0);
    // Negative and neutral, and the key is UNCONDITIONAL like its four
    // Basic-panel neighbours (Clarity2012 / Dehaze / Vibrance / Saturation).
    let neg = recipe_to_xmp(&EditRecipe { texture: -40.0, ..Default::default() });
    assert!(neg.contains(r#"crs:Texture="-40""#));
    assert_eq!(xmp_to_recipe(&neg).texture, -40.0);
    assert!(recipe_to_xmp(&EditRecipe::default()).contains(r#"crs:Texture="0""#));
    // A FOREIGN sidecar's Texture is a real import, not a disclosure line.
    let lr = "<rdf:Description rdf:about=\"\" \
                  xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
                  crs:Texture=\"+26\"/>";
    assert_eq!(xmp_to_recipe(lr).texture, 26.0, "the LR value must arrive");
}

/// R25 B2, the strip arm: a cleared Texture must DISAPPEAR from a merged
/// document rather than linger at Lightroom's old value.
///
/// This is what owning a key means. Before B2, `crs:Texture` was foreign
/// property and the merge preserved it verbatim (there are four tests
/// above that used it as the example of exactly that). Now the merge
/// strips it and rewrites ours — and if `owned_attr_keys` had gained the
/// key without the writer emitting it, or the writer without the key, this
/// document would answer one slider twice.
#[test]
fn a_cleared_texture_disappears_from_a_merged_document() {
    let lr = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n \
                  <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  \
                  <rdf:Description rdf:about=\"\"\n    \
                  xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n    \
                  crs:Texture=\"+20\" crs:PointColor=\"0\" crs:HasSettings=\"True\">\n  \
                  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n";
    let merged = merged_doc(lr, &EditRecipe { texture: -8.0, ..Default::default() })
        .expect("a plain LR sidecar is mergeable");
    assert_eq!(merged.matches("crs:Texture=").count(), 1, "one answer only: {merged}");
    assert!(merged.contains(r#"crs:Texture="-8""#), "…and it is OURS: {merged}");
    assert!(merged.contains("crs:PointColor=\"0\""), "an unmodelled global still survives");
    // Cleared to neutral: the old +20 is gone, not resurrected.
    let cleared = merged_doc(lr, &EditRecipe::default()).expect("mergeable");
    assert_eq!(cleared.matches("crs:Texture=").count(), 1);
    assert!(cleared.contains(r#"crs:Texture="0""#), "the stale +20 must not linger: {cleared}");
    assert_eq!(xmp_to_recipe(&cleared).texture, 0.0);
}

/// R25 B3: the eight carried DETAIL axes and the manual CA pair
/// round-trip, each in the spelling Lightroom itself uses.
///
/// The spellings are FIRST-HAND, from all seven sidecars in the user's
/// library: `SharpenRadius="+1.0"` carries an explicit sign and one
/// decimal, while every integer neighbour is bare (`SharpenDetail="25"`,
/// `SharpenEdgeMasking="0"`, `ColorNoiseReduction="25"`). Getting that
/// backwards is not cosmetic — it is the difference between a sidecar
/// Lightroom reads as its own and one it merely tolerates.
#[test]
fn detail_subcontrols_round_trip() {
    let r = EditRecipe {
        sharpen_radius: 1.0,
        sharpen_detail: 25.0,
        sharpen_mask: 12.0,
        nr_detail: 50.0,
        nr_contrast: 8.0,
        color_nr: 25.0,
        color_nr_detail: 50.0,
        color_nr_smooth: 50.0,
        ca_r: 14.0,
        ca_b: -9.0,
        auto_lateral_ca: true,
        ..Default::default()
    };
    let xmp = recipe_to_xmp(&r);
    for want in [
        r#"crs:SharpenRadius="+1.0""#,
        r#"crs:SharpenDetail="25""#,
        r#"crs:SharpenEdgeMasking="12""#,
        r#"crs:LuminanceNoiseReductionDetail="50""#,
        r#"crs:LuminanceNoiseReductionContrast="8""#,
        r#"crs:ColorNoiseReduction="25""#,
        r#"crs:ColorNoiseReductionDetail="50""#,
        r#"crs:ColorNoiseReductionSmoothness="50""#,
        r#"crs:ChromaticAberrationR="14""#,
        r#"crs:ChromaticAberrationB="-9""#,
        r#"crs:AutoLateralCA="1""#,
    ] {
        assert!(xmp.contains(want), "{want} missing from: {xmp}");
    }
    let back = xmp_to_recipe(&xmp);
    for (name, live, want) in [
        ("sharpen_radius", back.sharpen_radius, r.sharpen_radius),
        ("sharpen_detail", back.sharpen_detail, r.sharpen_detail),
        ("sharpen_mask", back.sharpen_mask, r.sharpen_mask),
        ("nr_detail", back.nr_detail, r.nr_detail),
        ("nr_contrast", back.nr_contrast, r.nr_contrast),
        ("color_nr", back.color_nr, r.color_nr),
        ("color_nr_detail", back.color_nr_detail, r.color_nr_detail),
        ("color_nr_smooth", back.color_nr_smooth, r.color_nr_smooth),
        ("ca_r", back.ca_r, r.ca_r),
        ("ca_b", back.ca_b, r.ca_b),
    ] {
        assert_eq!(live, want, "{name} did not survive the round trip");
    }
    assert!(back.auto_lateral_ca, "the auto-CA flag must come back on");
    // A neutral recipe writes NONE of them: an absent key is how
    // Lightroom is told to keep its own default (Radius 1.0, Detail 25,
    // Colour NR Detail/Smoothness 50/50), and inventing a zero for each
    // would be a change to the photo, not a faithful silence. The ONE
    // exception is `ColorNoiseReduction` itself (v1.3.1, `amount_carries`):
    // a recipe's zero is colour noise reduction OFF, which is what this
    // engine renders, and an absent key let Lightroom apply its RAW default
    // of 25 to a photo AutoShade showed without it (measured 2026-09-12).
    let neutral = recipe_to_xmp(&EditRecipe::default());
    for key in [
        "SharpenRadius",
        "SharpenDetail",
        "SharpenEdgeMasking",
        "LuminanceNoiseReduction",
        "ColorNoiseReductionDetail",
        "ColorNoiseReductionSmoothness",
        "ChromaticAberration",
        "AutoLateralCA",
    ] {
        assert!(!neutral.contains(key), "{key} must be absent from a neutral sidecar");
    }
    assert!(
        neutral.contains(r#"crs:ColorNoiseReduction="0""#),
        "colour NR goes out at the engine's value even at rest: {neutral}"
    );
    // …and the merge STRIPS them, so a cleared value cannot linger at
    // Lightroom's old number beside ours.
    let lr = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n \
                  <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  \
                  <rdf:Description rdf:about=\"\"\n    \
                  xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n    \
                  crs:SharpenRadius=\"+1.0\" crs:ColorNoiseReduction=\"25\" \
                  crs:AutoLateralCA=\"1\" crs:HasSettings=\"True\">\n  \
                  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n";
    assert_eq!(xmp_to_recipe(lr).color_nr, 25.0, "premise: the LR values import");
    assert!(xmp_to_recipe(lr).auto_lateral_ca, "premise: so does the flag");
    let cleared = merged_doc(lr, &EditRecipe::default()).expect("a plain LR sidecar merges");
    for key in ["SharpenRadius", "AutoLateralCA"] {
        assert!(!cleared.contains(key), "{key} survived a clear: {cleared}");
    }
    // …and the colour NR is REPLACED, not merely stripped: one answer,
    // and it is the engine's zero, never Lightroom's stale 25.
    assert_eq!(cleared.matches("crs:ColorNoiseReduction=").count(), 1, "one answer: {cleared}");
    assert!(cleared.contains(r#"crs:ColorNoiseReduction="0""#), "ours: {cleared}");
}

/// v1.6.0: the Sharpening amount is a companion — absent from the sidecar
/// while the recipe holds no value, so Lightroom applies its own default
/// for the kind of file (40 on a RAW, 0 on a JPEG), which is what the
/// engine renders (`EditRecipe::capture_sharpening`); a real 0 is stated.
///
/// MUTATIONS THIS CATCHES: the writer emitting `Sharpness="0"` for an
/// absent amount again (Lightroom then renders a RAW unsharpened that the
/// app shows at 40); the reader folding a stated `Sharpness="0"` into
/// "absent" (Lightroom's own 0 imports as 40 on a RAW).
#[test]
fn an_absent_sharpening_amount_leaves_the_sidecar_to_lightroom_and_a_real_zero_is_stated() {
    let absent = recipe_to_xmp(&EditRecipe::default());
    assert!(!absent.contains("crs:Sharpness="), "no amount, no key: {absent}");
    let fresh = xmp_to_recipe(&absent);
    assert!(!fresh.explicit_zero.iter().any(|n| n == "sharpening"), "{:?}", fresh.explicit_zero);
    assert_eq!((fresh.capture_sharpening(true), fresh.capture_sharpening(false)), (40.0, 0.0));

    let mut zero = EditRecipe::default();
    zero.set_resolved("sharpening", 0.0);
    let stated = recipe_to_xmp(&zero);
    assert!(stated.contains(r#"crs:Sharpness="0""#), "{stated}");
    let back = xmp_to_recipe(&stated);
    assert_eq!(back.explicit_zero, vec!["sharpening".to_string()]);
    assert_eq!((back.capture_sharpening(true), back.capture_sharpening(false)), (0.0, 0.0));

    // A stated amount is itself on both kinds, as before.
    let forty = xmp_to_recipe(&recipe_to_xmp(&EditRecipe { sharpening: 40.0, ..Default::default() }));
    assert_eq!((forty.capture_sharpening(true), forty.capture_sharpening(false)), (40.0, 40.0));
}

/// v1.5.0: a COMPANION key Lightroom states AT 0 is a value, an absent one
/// is Lightroom's default — the difference `explicit_zero` holds, in both
/// directions of the sidecar.
///
/// MUTATIONS THIS CATCHES: the reader's explicit-zero pass removed (an LR
/// Detail 0 imports as "absent" and renders at 25); the writer's `states`
/// reduced to `v != 0.0` (our real 0 leaves the sidecar and Lightroom
/// renders its default); the era gate's `untouched` blind to the list (a
/// legacy recipe's real 0 is suppressed on save).
#[test]
fn a_companion_stated_at_zero_is_a_value_and_an_absent_one_is_lightrooms_default() {
    let lr = "<rdf:Description rdf:about=\"\" \
                  xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
                  crs:Sharpness=\"40\" crs:SharpenRadius=\"+1.0\" crs:SharpenDetail=\"0\" \
                  crs:LuminanceSmoothing=\"30\" crs:LuminanceNoiseReductionDetail=\"50\" \
                  crs:GrainAmount=\"20\" crs:GrainSize=\"0\"/>";
    let r = xmp_to_recipe(lr);
    assert_eq!(
        r.explicit_zero,
        vec!["grain_size".to_string(), "sharpen_detail".to_string()],
        "exactly the companions stated at 0"
    );
    assert_eq!(r.resolved("sharpen_detail"), 0.0, "Lightroom's Detail 0 renders at 0");
    assert_eq!(r.resolved("nr_detail"), 50.0, "a stated default is itself");
    assert_eq!(r.resolved("color_nr_detail"), 50.0, "an absent one renders at the default");

    // Written back: the real zeros go out as zeros, the absent stays absent.
    let xmp = recipe_to_xmp(&r);
    assert!(xmp.contains(r#"crs:SharpenDetail="0""#), "{xmp}");
    assert!(xmp.contains(r#"crs:GrainSize="0""#), "{xmp}");
    assert!(!xmp.contains("ColorNoiseReductionDetail"), "never an invented zero: {xmp}");
    assert_eq!(xmp_to_recipe(&xmp).explicit_zero, r.explicit_zero, "and it reads back the same");

    // A LEGACY recipe (schema era 0) still writes a real zero the
    // photographer set: the era gate suppresses only what serde filled.
    let mut legacy = EditRecipe { schema_era: 0, sharpening: 40.0, ..Default::default() };
    legacy.set_resolved("sharpen_detail", 0.0);
    let written = recipe_to_xmp(&legacy);
    assert!(written.contains(r#"crs:SharpenDetail="0""#), "{written}");
    assert!(!written.contains("SharpenRadius"), "the absent radius stays absent: {written}");
}

/// v0.31.1: `crs:Sharpness` is Lightroom's Detail > Sharpening **Amount**
/// slider stored 1:1, and that slider's UI maximum is **150**, not 100.
///
/// EVIDENCE (web survey of GitHub-hosted `.xmp`, 2026-08-18; 566
/// `crs:Sharpness` occurrences across 636 files, histogram maximum 150):
/// fifteen REAL sidecars carry `crs:Sharpness="150"` — fourteen from
/// `maxbordogna/finger_counting` (`tiff:Model="NIKON Z 6"`,
/// `crs:Version="15.3"` / `ProcessVersion="11.0"`) and one from
/// `ninjahisser/Fotos` (`NIKON Z 30`, `crs:Version="17.2"` /
/// `ProcessVersion="15.4"`), the latter with the value sitting inside the
/// Detail attribute group beside `SharpenRadius="+1.0"` /
/// `SharpenDetail="25"` / `SharpenEdgeMasking="0"` in a file that names its
/// own raw. Two repositories, two camera bodies, two Lightroom
/// generations. The sidecar values are also copied here as TEXT ONLY —
/// no harvested file enters this repository.
///
/// The 0..100 belief was load-bearing in four places, all deleted with it:
/// the reader's ×1.5, the writer's ×⅔, the `Sharpness` special case in
/// `crs_number_is_in_recipe_range`, and an assertion that PINNED
/// `100 × 1.5 == 150` as an invariant. This test is the replacement pin —
/// it fails on every one of those four.
#[test]
fn a_full_lightroom_sharpening_amount_imports_as_itself() {
    // The maximum a real Lightroom writes. Synthetic document, real value.
    let doc = |v: &str| {
        format!(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n <rdf:RDF \
                 xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  \
                 <rdf:Description rdf:about=\"\"\n    \
                 xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n    \
                 crs:Sharpness=\"{v}\" crs:SharpenRadius=\"+1.0\" crs:SharpenDetail=\"25\"\n    \
                 crs:SharpenEdgeMasking=\"0\" crs:HasSettings=\"True\">\n  \
                 </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n"
        )
    };
    // 1. A full 150 arrives as 150. Under the old reader it was 225,
    //    clamped back to 150 by luck — and under the old BAND it was also
    //    reported as an unparsable number, i.e. the app told the user a
    //    perfectly ordinary Lightroom file was broken.
    let full = xmp_to_recipe(&doc("150"));
    assert_eq!(full.sharpening, 150.0, "a full Lightroom Amount is 150 here too");
    assert!(
        unparsable_crs_numbers(&doc("150")).is_empty(),
        "150 is an ordinary value, not a defect: {:?}",
        unparsable_crs_numbers(&doc("150"))
    );
    // 2. The user's own library value. 40 in, 40 out — it used to be 60.
    assert_eq!(xmp_to_recipe(&doc("40")).sharpening, 40.0);
    // 3. WYSIWYG on the way back out: what the engine renders is what the
    //    sidecar says. A rendered 60 used to be written back as 40.
    let sixty = EditRecipe { sharpening: 60.0, ..Default::default() };
    assert!(
        recipe_to_xmp(&sixty).contains(r#"crs:Sharpness="60""#),
        "{}",
        recipe_to_xmp(&sixty)
    );
    // …and 150 survives a full round trip, which the old ×⅔ writer could
    // not do at all: it had no way to SAY 150.
    assert!(recipe_to_xmp(&full).contains(r#"crs:Sharpness="150""#));
    assert_eq!(xmp_to_recipe(&recipe_to_xmp(&full)).sharpening, 150.0);
    // 4. The band still has a ceiling, and it is the row's: 151 is out.
    assert!(
        unparsable_crs_numbers(&doc("151")).iter().any(|k| k == "Sharpness"),
        "past the slider's own maximum is still disclosed"
    );
}

/// R25 B3 (policy SF4-C): de-fringe round-trips through BOTH spellings
/// Lightroom writes — the `rdf:Description` attribute form and the
/// property-ELEMENT form.
///
/// `crs_str` already reads both (that is why the reader adds no third
/// scanning arm), and this is the test that keeps it that way. Positive
/// amounts are UNSIGNED: `DefringePurpleAmount="3"`, never `"+3"` — the
/// `Sharpness="40"` family, not the `Contrast2012="+22"` one.
#[test]
fn defringe_round_trips_both_serialization_forms() {
    let r = EditRecipe {
        defringe_purple: 3.0,
        defringe_purple_lo: 39.0,
        defringe_purple_hi: 79.0,
        ..Default::default()
    };
    let xmp = recipe_to_xmp(&r);
    assert!(xmp.contains(r#"crs:DefringePurpleAmount="3""#), "unsigned, like Sharpness: {xmp}");
    assert!(!xmp.contains(r#"DefringePurpleAmount="+3""#), "no `+` on this family");
    let back = xmp_to_recipe(&xmp);
    assert_eq!(
        (back.defringe_purple, back.defringe_purple_lo, back.defringe_purple_hi),
        (3.0, 39.0, 79.0)
    );
    // The ELEMENT form, in the wild on photoprism's canon_eos_6d fixture
    // family (alphabetical, one child per key).
    let elem = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n \
                    <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  \
                    <rdf:Description rdf:about=\"\"\n    \
                    xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\">\n   \
                    <crs:DefringeGreenAmount>5</crs:DefringeGreenAmount>\n   \
                    <crs:DefringeGreenHueHi>66</crs:DefringeGreenHueHi>\n   \
                    <crs:DefringeGreenHueLo>44</crs:DefringeGreenHueLo>\n   \
                    <crs:DefringePurpleAmount>7</crs:DefringePurpleAmount>\n  \
                    </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n";
    let e = xmp_to_recipe(elem);
    assert_eq!(
        (e.defringe_green, e.defringe_green_lo, e.defringe_green_hi, e.defringe_purple),
        (5.0, 44.0, 66.0, 7.0),
        "the element form must read exactly like the attribute form"
    );
    // The purple WINDOW was not named by that document — it must fall
    // back to Adobe's default, not to the zero `crs_f32` answers.
    assert_eq!(
        (e.defringe_purple_lo, e.defringe_purple_hi),
        (30.0, 70.0),
        "an unnamed hue window is Adobe's default, never 0..0"
    );
}

/// R25 B3: a NON-default hue window survives — the case the fallback
/// above must not swallow.
///
/// 39/79 is the real shape from a Lightroom preset (`lightA1`): the
/// amount at 3 with the window moved off 30/70. If the reader ever
/// "normalised" a window it did not recognise, this is what would be
/// silently rewritten to Adobe's default on the next save.
#[test]
fn nondefault_hue_bounds_survive() {
    let lr = "<rdf:Description rdf:about=\"\" \
                  xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
                  crs:DefringePurpleAmount=\"3\" crs:DefringePurpleHueLo=\"39\" \
                  crs:DefringePurpleHueHi=\"79\"/>";
    let r = xmp_to_recipe(lr);
    assert_eq!((r.defringe_purple_lo, r.defringe_purple_hi), (39.0, 79.0));
    let round = xmp_to_recipe(&recipe_to_xmp(&r));
    assert_eq!(
        (round.defringe_purple, round.defringe_purple_lo, round.defringe_purple_hi),
        (3.0, 39.0, 79.0),
        "a moved window must not snap back to 30/70 on a save"
    );
    // The green half of the same document said nothing, so it comes back
    // as Adobe's 40/60 — and that is what gets written, which is the
    // shape every real sidecar has.
    assert_eq!((round.defringe_green_lo, round.defringe_green_hi), (40.0, 60.0));
    assert!(recipe_to_xmp(&r).contains(r#"crs:DefringeGreenHueLo="40""#));
}

/// R25 B3: the whole de-fringe block is written UNCONDITIONALLY, and a
/// document carrying exactly Adobe's defaults imports as a NO-OP.
///
/// Both halves of the non-zero-neutral decision in one place. The first
/// is the shape Lightroom writes — 7 of 7 of the user's sidecars carry
/// all six keys with the amounts at 0 — so a recipe that says nothing
/// still produces a document that looks like one Lightroom made. The
/// second is what stops that from making every photo "edited": the six
/// values are `EditRecipe::default()`'s own, so `is_noop` still answers
/// yes. A reader that took `crs_f32`'s absent-key zero instead would
/// import a 0..0 hue window and fail this, which is exactly the bug the
/// fallback exists to prevent.
#[test]
fn a_real_defringe_block_imports_as_a_noop() {
    let neutral = recipe_to_xmp(&EditRecipe::default());
    for want in [
        r#"crs:DefringePurpleAmount="0""#,
        r#"crs:DefringePurpleHueLo="30""#,
        r#"crs:DefringePurpleHueHi="70""#,
        r#"crs:DefringeGreenAmount="0""#,
        r#"crs:DefringeGreenHueLo="40""#,
        r#"crs:DefringeGreenHueHi="60""#,
    ] {
        assert!(neutral.contains(want), "{want} missing — the block is unconditional: {neutral}");
    }
    // The real shape, verbatim from the user's library (P50 line 139
    // onward), on an otherwise empty document.
    let real = "<rdf:Description rdf:about=\"\" \
                    xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
                    crs:DefringePurpleAmount=\"0\" crs:DefringePurpleHueLo=\"30\" \
                    crs:DefringePurpleHueHi=\"70\" crs:DefringeGreenAmount=\"0\" \
                    crs:DefringeGreenHueLo=\"40\" crs:DefringeGreenHueHi=\"60\"/>";
    assert!(
        xmp_to_recipe(real).is_noop(),
        "a sidecar carrying only Adobe's own de-fringe defaults is not an edit"
    );
    // …and so is a document that never mentions de-fringe at all — the
    // OTHER direction of the same fallback.
    let silent = "<rdf:Description rdf:about=\"\" \
                      xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"/>";
    assert!(xmp_to_recipe(silent).is_noop(), "no de-fringe keys is no edit either");
    // A REAL de-fringe, by contrast, is one.
    let edited = real.replace("crs:DefringePurpleAmount=\"0\"", "crs:DefringePurpleAmount=\"3\"");
    assert!(!xmp_to_recipe(&edited).is_noop(), "an actual de-fringe IS an edit");
}

/// R25 B3, the complement arm: the detail block, the CA pair and
/// de-fringe have all LEFT `unmodelled_global_crs` — with no edit to that
/// function, because its universe is the complement of `owned_attr_keys`.
///
/// This is the same "the list shrinks by itself" property B2 proved for
/// Texture, and it is the reason the writer and the reader had to land in
/// one commit: a key we read but never write would still be foreign
/// property, named here and duplicated by the merge.
#[test]
fn unmodelled_list_no_longer_names_the_detail_block() {
    let lr = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n \
                  <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  \
                  <rdf:Description rdf:about=\"\"\n    \
                  xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n    \
                  crs:SharpenRadius=\"+1.0\" crs:SharpenDetail=\"25\" \
                  crs:SharpenEdgeMasking=\"0\" crs:LuminanceNoiseReductionDetail=\"50\" \
                  crs:LuminanceNoiseReductionContrast=\"0\" crs:ColorNoiseReduction=\"25\" \
                  crs:ColorNoiseReductionDetail=\"50\" crs:ColorNoiseReductionSmoothness=\"50\" \
                  crs:AutoLateralCA=\"1\" crs:ChromaticAberrationR=\"0\" \
                  crs:ChromaticAberrationB=\"0\" crs:DefringePurpleAmount=\"3\" \
                  crs:DefringePurpleHueLo=\"39\" crs:DefringePurpleHueHi=\"79\" \
                  crs:DefringeGreenAmount=\"0\" crs:DefringeGreenHueLo=\"40\" \
                  crs:DefringeGreenHueHi=\"60\" crs:PointColor=\"0\" \
                  crs:HasSettings=\"True\">\n  \
                  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n";
    let found = unmodelled_global_crs(lr);
    for gone in [
        "SharpenRadius",
        "SharpenDetail",
        "SharpenEdgeMasking",
        "LuminanceNoiseReductionDetail",
        "LuminanceNoiseReductionContrast",
        "ColorNoiseReduction",
        "ColorNoiseReductionDetail",
        "ColorNoiseReductionSmoothness",
        "AutoLateralCA",
        "ChromaticAberrationR",
        "ChromaticAberrationB",
        "DefringePurpleAmount",
        "DefringePurpleHueLo",
        "DefringePurpleHueHi",
        "DefringeGreenAmount",
        "DefringeGreenHueLo",
        "DefringeGreenHueHi",
    ] {
        assert!(
            !found.contains(&gone.to_string()),
            "{gone} is modelled since R25 B3 and must have left the list: {found:?}"
        );
    }
    // The premise: the scan really did run and still names what we do
    // NOT model, or the seventeen assertions above prove nothing.
    assert!(
        found.contains(&"PointColor".to_string()),
        "an unmodelled global must still be named: {found:?}"
    );
    // …and the values arrived rather than merely stopping being foreign.
    let r = xmp_to_recipe(lr);
    assert_eq!((r.sharpen_radius, r.color_nr, r.defringe_purple), (1.0, 25.0, 3.0));
    assert!(r.auto_lateral_ca);
}

// ───────────────────── R25 B4: the pass-through blocks ──────────────────

/// A Lightroom Transform block, its Upright solver's own bookkeeping, and a
/// camera profile — synthetic, but every value below is copied CHARACTER
/// FOR CHARACTER out of the operator's reference sidecars (a bare `0`, a
/// decimal `0.00`, a signed `+0.9`, a plain `100`, a negative `-35`, two
/// nine-digit normalised fractions, a focal length past the registry's
/// fallback band, and a profile NAME with a space in it: nine different
/// spellings of things a number formatter would flatten into three).
///
/// FIXTURE NOTE, F6 REVISION. The eight `crs:Perspective*` keys are OWNED
/// controls since v1.5.0, so they are no longer this document's
/// pass-through sample — the six `Upright*` keys beside them are, and
/// Lightroom writes those on every photo its Upright panel has touched.
/// Keeping the Perspective keys here is the point: the same document now
/// exercises both sides of the line.
fn lr_transform_doc() -> String {
    "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
         xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
         <rdf:Description rdf:about=\"\" \
         xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
         crs:Exposure2012=\"+1.00\" \
         crs:PerspectiveUpright=\"0\" crs:PerspectiveVertical=\"-35\" \
         crs:PerspectiveHorizontal=\"0\" crs:PerspectiveRotate=\"+0.9\" \
         crs:PerspectiveScale=\"100\" crs:PerspectiveAspect=\"0\" \
         crs:PerspectiveX=\"0.00\" crs:PerspectiveY=\"0.00\" \
         crs:UprightVersion=\"151388160\" crs:UprightCenterMode=\"0\" \
         crs:UprightCenterNormX=\"0.422764964\" \
         crs:UprightCenterNormY=\"0.46112117\" \
         crs:UprightFocalMode=\"0\" \
         crs:UprightFocalLength35mm=\"104.944461871\" \
         crs:CameraProfile=\"Adobe Standard\" \
         crs:CameraProfileDigest=\"2D1D4700365C3E2831EEAE0D1A8F9CDF\" \
         crs:HasSettings=\"True\"/></rdf:RDF></x:xmpmeta>"
        .to_string()
}

/// **The first real payload `Tier::PassThrough` has ever carried**
/// (`ARCHITECTURE.md` recorded the tier as "none yet" until this batch).
/// The registry's three-sided law only checks that a ROW exists; this
/// checks that the value survives the round trip as ITSELF.
#[test]
fn passthrough_round_trips_verbatim() {
    let r = xmp_to_recipe(&lr_transform_doc());
    assert_eq!(
        r.passthrough.len(),
        6,
        "the six Upright bookkeeping keys, and nothing else: {:?}",
        r.passthrough
    );
    // VERBATIM means the spelling too. Every one of these would have been
    // destroyed by a number round trip: nine significant digits do not
    // survive an f32 format, 104.944461871 is outside the registry's
    // fallback band, and `Adobe Standard` is not a number at all.
    for (key, want) in [
        ("UprightVersion", "151388160"),
        ("UprightCenterNormX", "0.422764964"),
        ("UprightCenterNormY", "0.46112117"),
        ("UprightFocalLength35mm", "104.944461871"),
    ] {
        assert_eq!(r.passthrough.get(key).map(String::as_str), Some(want), "{key}");
    }
    // The profile NAME crossed the same line in F7, and its spelling
    // matters for the same reason: it is an identifier that has to keep
    // matching the file it names.
    assert_eq!(r.camera_profile, "Adobe Standard");
    assert!(!r.passthrough.contains_key("CameraProfile"), "an owned key is not carried");
    // The eight Perspective keys crossed the line in v1.5.0 F6: OWNED, so
    // they are parsed into their own fields and are NOT in this map.
    for owned in ["PerspectiveVertical", "PerspectiveRotate", "PerspectiveX"] {
        assert!(!r.passthrough.contains_key(owned), "{owned} is an owned control now");
    }
    assert_eq!(r.perspective_vertical, -35.0, "…and it arrived as a number");
    assert_eq!(r.perspective_rotate, 0.9, "…including the signed one");
    // The keys OUTSIDE the named nine are untouched by all of this:
    // the digest stays foreign, preserved by the merge and named by the
    // import disclosure. "Named set, not everything unknown."
    assert!(!r.passthrough.contains_key("CameraProfileDigest"));
    assert!(
        unmodelled_global_crs(&lr_transform_doc()).contains(&"CameraProfileDigest".to_string())
    );

    // Out and back, through OUR writer.
    let ours = recipe_to_xmp(&r);
    assert!(ours.contains(r#"crs:UprightFocalLength35mm="104.944461871""#), "{ours}");
    assert!(ours.contains(r#"crs:CameraProfile="Adobe Standard""#), "{ours}");
    assert_eq!(xmp_to_recipe(&ours).passthrough, r.passthrough, "a full verbatim round trip");
    // The owned half round-trips too, in its own measured spellings.
    assert!(ours.contains(r#"crs:PerspectiveRotate="+0.9""#), "{ours}");
    assert!(ours.contains(r#"crs:PerspectiveVertical="-35""#), "{ours}");

    // Written in PASSTHROUGH_CRS order, not the BTreeMap's alphabetical
    // one: Adobe writes its solver's block before the profile name, and a
    // diff against Lightroom's own file has to be readable.
    let at = |k: &str| ours.find(&format!("crs:{k}=")).unwrap_or_else(|| panic!("{k} missing"));
    assert!(at("UprightVersion") < at("UprightFocalLength35mm"), "the block keeps its order");
    assert!(at("UprightFocalLength35mm") < at("CameraProfile"), "block before the profile name");

    // XML transport still applies — escaping is not interpretation, and a
    // profile name really can carry an ampersand.
    let odd = EditRecipe {
        camera_profile: "Sky & Sea <v2>".to_string(),
        ..Default::default()
    };
    let doc = recipe_to_xmp(&odd);
    assert!(doc.contains("Sky &amp; Sea &lt;v2&gt;"), "escaped on the way out: {doc}");
    assert_eq!(
        xmp_to_recipe(&doc).camera_profile,
        "Sky & Sea <v2>",
        "…and unescaped back to the very same string"
    );
}

/// The empty map is the state of every recipe written before this batch,
/// and it must change NOTHING: no invented Transform block in the sidecar,
/// and an older `recipe.json` with no such key still reads.
#[test]
fn an_empty_passthrough_map_leaves_the_sidecar_bytes_unchanged() {
    let doc = recipe_to_xmp(&EditRecipe::default());
    for key in PASSTHROUGH_CRS {
        assert!(
            !doc.contains(&format!("crs:{key}=")),
            "{key}: a recipe that never saw a Transform block must not assert one: {doc}"
        );
    }
    // Forward/backward compatibility of the FIELD, which is the other
    // half of "unchanged": a recipe.json from v0.30 has no `passthrough`
    // key at all and must still load, as an empty map.
    let legacy = r#"{"version":2,"exposure_ev":0.25}"#;
    let r: EditRecipe = serde_json::from_str(legacy).expect("a legacy recipe still parses");
    assert!(r.passthrough.is_empty());
    assert_eq!(r.exposure_ev, 0.25);
}

/// The merge's own trap, and the reason [`merge_strip_keys`] exists: a
/// recipe that never SAW a Transform block must not delete one.
///
/// Owning a key normally licenses the strip — that is how a cleared
/// vignette disappears. Pass-through has no cleared state: nothing in the
/// app can empty the map, so "absent" only ever means "this recipe came
/// from somewhere that never read the document" (a v0.30 recipe.json, a
/// paste from another photo, a fresh Analyze). Stripping on that would
/// delete the photographer's Upright correction and camera profile from
/// the file beside their RAW on an ordinary Ctrl+S.
///
/// v1.5.0 F6 SPLIT THIS TEST IN TWO, because the block it is named after
/// now has two halves with two different protections:
///
/// * the six Upright bookkeeping keys are still carried, and
///   `merge_strip_keys` is still what saves them — case (a). The profile
///   NAME left that half in F7 and is protected differently again: it is
///   owned, and an empty name means "not stated" rather than "cleared"
///   (`unspoken_attr_keys`);
/// * the eight `crs:Perspective*` keys are OWNED, so a build that has them
///   overwrites Lightroom's exactly as it overwrites `crs:Exposure2012`,
///   and what protects a recipe written before they existed is the SCHEMA
///   ERA, not the strip list — case (a2).
///
/// Reading the second half as a regression would have been the easy
/// mistake: an owned key that never overwrites is a control that renders
/// nothing.
#[test]
fn a_recipe_that_never_saw_a_transform_block_does_not_delete_one() {
    let lr = lr_transform_doc();
    // (a) The dangerous case: empty map, real Upright bookkeeping in the
    // base. THIS is what the strip list protects.
    let blind = EditRecipe { exposure_ev: 0.25, ..Default::default() };
    let merged = merged_doc(&lr, &blind).expect("mergeable");
    for want in [
        r#"crs:UprightCenterNormX="0.422764964""#,
        r#"crs:UprightFocalLength35mm="104.944461871""#,
        r#"crs:CameraProfile="Adobe Standard""#,
    ] {
        assert!(merged.contains(want), "the base's own {want} must survive: {merged}");
    }
    assert_eq!(merged.matches("crs:CameraProfile=").count(), 1, "and exactly once");
    assert!(merged.contains(r#"crs:Exposure2012="0.25""#), "…while ours still publish");
    // …and the OWNED half published ours, which is what owning means. The
    // recipe is era 2, so it has an opinion about the keystone: none.
    assert_eq!(blind.schema_era, crate::recipe::SCHEMA_ERA, "premise: a current build");
    assert!(
        !merged.contains(r#"crs:PerspectiveVertical="-35""#),
        "an era-2 recipe states its own Transform, like its own Exposure: {merged}"
    );

    // (a2) The same dangerous case for the owned half: a recipe written
    // BEFORE v1.5.0 has never held a Perspective key, so the era gate
    // neither strips nor re-emits one and Lightroom's own bytes stand.
    let era1 = EditRecipe { exposure_ev: 0.25, schema_era: 1, ..Default::default() };
    let merged = merged_doc(&lr, &era1).expect("mergeable");
    for want in [
        r#"crs:PerspectiveVertical="-35""#,
        r#"crs:PerspectiveRotate="+0.9""#,
        r#"crs:CameraProfile="Adobe Standard""#,
    ] {
        assert!(merged.contains(want), "an era-1 recipe must leave {want} alone: {merged}");
    }
    assert!(merged.contains(r#"crs:Exposure2012="0.25""#), "…while an era-0 key still does");

    // (b) The ordinary case: the recipe DID read the block, so ours are
    // stripped and rewritten — one copy, never two. Probed on a key that
    // is OFF NEUTRAL in the document, because a neutral one has nothing to
    // rewrite and would prove the claim by accident.
    let seen = EditRecipe { exposure_ev: 0.25, ..xmp_to_recipe(&lr) };
    let merged = merged_doc(&lr, &seen).expect("mergeable");
    assert_eq!(merged.matches("crs:CameraProfile=").count(), 1, "stripped, then rewritten");
    assert_eq!(merged.matches("crs:PerspectiveVertical=").count(), 1, "the owned half too");
    assert!(merged.contains(r#"crs:PerspectiveVertical="-35""#), "{merged}");
    assert!(merged.contains(r#"crs:CameraProfile="Adobe Standard""#));
    // …and the neutral one leaves NO key, which is what every owned control
    // at rest does (it is how a cleared vignette disappears). Harmless
    // here, and measured rather than assumed: this recipe's own default is
    // 100 and `xmp_to_recipe` reads an absent `PerspectiveScale` back as
    // 100, so the document renders the same in both apps with the key gone.
    assert_eq!(merged.matches("crs:PerspectiveScale=").count(), 0, "at rest, so not restated");
    assert_eq!(xmp_to_recipe(&merged).perspective_scale, 100.0, "…and absent still reads 100");
    // (c) …and a CHANGED value replaces rather than duplicates. Probed on
    // the OWNED profile name (v1.5.0 F7), because that is now the key with
    // both a strip and a write behind it — the carried half above is copied
    // verbatim and could never duplicate by a formatting difference.
    let mut edited = seen.clone();
    edited.camera_profile = "Adobe Landscape".to_string();
    let merged = merged_doc(&lr, &edited).expect("mergeable");
    assert_eq!(merged.matches("crs:CameraProfile=").count(), 1);
    assert!(merged.contains(r#"crs:CameraProfile="Adobe Landscape""#), "{merged}");
    assert!(!merged.contains("Adobe Standard\""), "the old value is gone: {merged}");
    // …and an UNSTATED name is not a cleared one: a recipe holding no
    // profile leaves Lightroom's own alone rather than deleting it.
    let mut silent = seen.clone();
    silent.camera_profile.clear();
    let kept = merged_doc(&lr, &silent).expect("mergeable");
    assert!(
        kept.contains(r#"crs:CameraProfile="Adobe Standard""#),
        "an empty name must not delete the photographer's profile: {kept}"
    );
}

/// The regenerate path — the one that "carries none of the base's
/// properties". It carries these: the seven live in the RECIPE now, so a
/// document rebuilt from scratch still states them. (`pipeline`'s
/// regeneration note names the creative `Look` instead of the camera
/// profile for exactly this reason.)
///
/// Both halves of the F6 block, because after v1.5.0 they reach a fresh
/// document by two different routes and the same assertion would have
/// passed on either one alone: the carried keys ride the `passthrough`
/// map, the eight `crs:Perspective*` keys ride their own owned fields.
#[test]
fn passthrough_survives_a_regenerate() {
    let r = xmp_to_recipe(&lr_transform_doc());
    let fresh = recipe_to_xmp(&r); // no base document at all
    assert!(fresh.contains(r#"crs:CameraProfile="Adobe Standard""#), "{fresh}");
    assert!(fresh.contains(r#"crs:UprightFocalLength35mm="104.944461871""#), "{fresh}");
    assert_eq!(xmp_to_recipe(&fresh).passthrough, r.passthrough);
    // The owned half, by its own route: a number this time, formatted by
    // the writer rather than copied as a string.
    assert!(fresh.contains(r#"crs:PerspectiveVertical="-35""#), "{fresh}");
    assert_eq!(xmp_to_recipe(&fresh).perspective_rotate, r.perspective_rotate);
}

/// Both spellings, one scanner. `crs_str` already reads the
/// property-ELEMENT form, so the reader needed no second scan arm (R24
/// round-end MED-1: a third arm is how the two forms drift apart) — and
/// the SCOPE rule matters more here than anywhere: a creative Look nests
/// its own baked `crs:CameraProfile`, and a flat scan would import the
/// PROFILE's name as the photographer's choice.
///
/// The profile name is an OWNED control since v1.5.0 F7, so the assertion
/// reads `camera_profile` rather than the carried map — the same question
/// about the same scope, asked of the field that now answers it.
#[test]
fn passthrough_reads_the_element_form_and_never_the_nested_look() {
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                   xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
                   <rdf:Description rdf:about=\"\" \
                   xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\">\
                   <crs:CameraProfile>Adobe Standard</crs:CameraProfile>\
                   <crs:UprightFocalLength35mm>104.944461871</crs:UprightFocalLength35mm>\
                   <crs:Look><rdf:Description><crs:Parameters><rdf:Description>\
                   <crs:CameraProfile>Camera Landscape</crs:CameraProfile>\
                   <crs:UprightFocalLength35mm>999</crs:UprightFocalLength35mm>\
                   </rdf:Description></crs:Parameters></rdf:Description></crs:Look>\
                   </rdf:Description></rdf:RDF></x:xmpmeta>";
    let r = xmp_to_recipe(doc);
    assert_eq!(
        r.camera_profile,
        "Adobe Standard",
        "the Description's OWN profile, never the Look's baked one"
    );
    assert_eq!(
        r.passthrough.get("UprightFocalLength35mm").map(String::as_str),
        Some("104.944461871")
    );
    // A document with no such block reports none — absence stays absence.
    assert!(xmp_to_recipe("<rdf:Description \
             xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
             crs:Exposure2012=\"0.00\"/>")
        .passthrough
        .is_empty());
}

/// A creative Look, as Lightroom 9.4 actually writes one.
///
/// Trimmed VERBATIM from the library's "Adobe Landscape" files — the shape
/// with baked sliders, which is the one that exercises every field. The
/// three `ToneCurvePV2012Red/Green/Blue` children Adobe writes beside the
/// master curve are identity on every file measured and are left out here
/// so the master curve's own assertion cannot pass by reading a sibling.
const LOOK_DOC: &str = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
         xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
         <rdf:Description rdf:about=\"\" \
         xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
         crs:Exposure2012=\"+1.00\" crs:Clarity2012=\"+4\" \
         crs:CameraProfile=\"Adobe Standard\">\
         <crs:Look>\
          <rdf:Description crs:Name=\"Adobe Landscape\" crs:Amount=\"1\" \
           crs:UUID=\"6F9C877E84273F4E8271E6B91BEB36A1\" \
           crs:SupportsMonochrome=\"false\">\
          <crs:Group><rdf:Alt><rdf:li xml:lang=\"x-default\">Profiles</rdf:li>\
          </rdf:Alt></crs:Group>\
          <crs:Parameters>\
           <rdf:Description crs:Version=\"18.0\" crs:ProcessVersion=\"15.4\" \
            crs:Highlights2012=\"-12\" crs:Shadows2012=\"+12\" \
            crs:Clarity2012=\"+10\" crs:ConvertToGrayscale=\"False\" \
            crs:CameraProfile=\"Adobe Standard\" \
            crs:LookTable=\"0B3BFB5CFB7DBF7FF175E98F24D316B0\">\
           <crs:ToneCurvePV2012><rdf:Seq>\
            <rdf:li>0, 0</rdf:li><rdf:li>64, 60</rdf:li>\
            <rdf:li>128, 128</rdf:li><rdf:li>192, 196</rdf:li>\
            <rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012>\
           </rdf:Description>\
          </crs:Parameters>\
          </rdf:Description>\
         </crs:Look>\
         </rdf:Description></rdf:RDF></x:xmpmeta>";

/// The creative Look is READ — its baked half reaches the recipe, and its
/// baked half only.
///
/// The line this pins is the one the whole F7 model rests on: the Look's
/// `Clarity2012="+10"` is the PROFILE's, the Description's own `"+4"` is
/// the PHOTOGRAPHER's, and they are two different numbers living in two
/// different fields. A reader that flattened the document would hand the
/// profile's value to the slider and show the photographer a clarity they
/// never set.
///
/// MUTATION: read the Look through the Description's scope, drop any field
/// from `read_creative_look`, or let an absent `crs:Amount` mean 0.
#[test]
fn a_creative_look_is_read_for_its_baked_half_and_nothing_else() {
    let r = xmp_to_recipe(LOOK_DOC);
    let look = r.look.as_ref().expect("the document carries a Look");
    assert_eq!(look.name, "Adobe Landscape");
    assert_eq!(look.amount, 1.0);
    assert_eq!(look.base_profile, "Adobe Standard");
    assert_eq!(look.table, "0B3BFB5CFB7DBF7FF175E98F24D316B0");
    assert!(!look.grayscale, "this Look is a colour one");
    assert_eq!((look.highlights, look.shadows, look.clarity), (-12.0, 12.0, 10.0));
    assert_eq!(look.tone_curve.len(), 5, "the baked curve: {:?}", look.tone_curve);
    assert_eq!(look.tone_curve[1], CurvePoint { input: 64, output: 60 });
    assert!(look.red_curve.is_empty() && look.blue_curve.is_empty(), "absent stays absent");

    // …and the two halves do not leak into each other.
    assert_eq!(r.clarity, 4.0, "the photographer's own clarity, not the profile's");
    assert_eq!(r.highlights, 0.0, "the profile's baked -12 is NOT a slider the user set");
    assert!(r.tone_curve.is_empty(), "…and its baked curve is not the user's curve");
    assert_eq!(r.camera_profile, "Adobe Standard");

    // The table is the one thing that cannot be rendered, so it is NAMED.
    assert_eq!(look.unrendered, vec!["LookTable".to_string()], "{:?}", look.unrendered);
    assert!(!look.is_neutral(), "a Look with baked moves is not neutral");
}

/// A document with no Look carries none, and a Look with nothing baked is
/// neutral rather than absent.
///
/// The difference matters downstream: `baked_look` filters on neutrality,
/// so a profile that only names a colour table must not reach the tone
/// composition and silently replace the engine's own base curve with an
/// empty one.
///
/// MUTATION: return `Some(Default)` for a document with no Look, or drop
/// the neutrality filter in `EditRecipe::baked_look`.
#[test]
fn a_look_with_nothing_baked_is_neutral_and_no_look_at_all_is_none() {
    let bare = LOOK_DOC
        .replace("crs:Highlights2012=\\\"-12\\\" ", "")
        .replace("crs:Shadows2012=\\\"+12\\\" ", "")
        .replace("crs:Clarity2012=\\\"+10\\\" ", "");
    let bare = bare
        .replace("crs:Highlights2012=\"-12\" ", "")
        .replace("crs:Shadows2012=\"+12\" ", "")
        .replace("crs:Clarity2012=\"+10\" ", "");
    let stripped = {
        let start = bare.find("<crs:ToneCurvePV2012>").expect("the curve");
        let end = bare.find("</crs:ToneCurvePV2012>").expect("the curve end")
            + "</crs:ToneCurvePV2012>".len();
        format!("{}{}", &bare[..start], &bare[end..])
    };
    let r = xmp_to_recipe(&stripped);
    let look = r.look.as_ref().expect("the Look element is still there");
    assert!(look.is_neutral(), "nothing baked: {look:?}");
    assert!(r.baked_look().is_none(), "a neutral Look does not reach the render");
    assert_eq!(look.table, "0B3BFB5CFB7DBF7FF175E98F24D316B0", "…but its table is still named");

    // An ABSENT `crs:Amount` is the whole Look, not none of it. The two
    // readings are one token apart and they differ by everything: amount 0
    // makes `is_neutral` true, so a profile that bakes a real curve would
    // be switched off silently rather than rendered. Probed here because
    // neutrality is exactly what an amount of 0 would forge.
    let no_amount = LOOK_DOC.replace(" crs:Amount=\"1\"", "");
    assert!(!no_amount.contains("crs:Amount"), "the attribute really went: {no_amount}");
    let silent = xmp_to_recipe(&no_amount);
    let unstated = silent.look.as_ref().expect("a Look with no stated amount is still a Look");
    assert_eq!(unstated.amount, 1.0, "absent means the whole Look");
    assert!(!unstated.is_neutral(), "…so its baked curve still renders");
    assert!(silent.baked_look().is_some(), "…and still reaches the render");

    // No Look element at all.
    let none = xmp_to_recipe(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
             xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
             <rdf:Description rdf:about=\"\" \
             xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
             crs:Exposure2012=\"0.00\"/></rdf:RDF></x:xmpmeta>",
    );
    assert!(none.look.is_none(), "no element, no Look");
    assert!(!none.renders_grayscale(), "and no black-and-white either");
}

/// A monochrome creative profile renders as black and white.
///
/// This is how Lightroom 9.4 spells a B&W treatment — four of the 175
/// sidecars measured carry `crs:Look` "Adobe Monochrome" whose Parameters
/// hold `ConvertToGrayscale="True"`, while the Description's own switch
/// stays absent. An engine that read only the top-level switch rendered
/// those four photographs in colour.
///
/// MUTATION: read `convert_to_grayscale` alone in `renders_grayscale`.
#[test]
fn a_monochrome_look_renders_black_and_white() {
    let mono = LOOK_DOC
        .replace("crs:ConvertToGrayscale=\"False\"", "crs:ConvertToGrayscale=\"True\"")
        .replace("Adobe Landscape", "Adobe Monochrome");
    let r = xmp_to_recipe(&mono);
    assert!(r.look.as_ref().expect("a Look").grayscale, "the Look's own switch");
    assert!(!r.convert_to_grayscale, "the photographer never touched theirs");
    assert!(r.renders_grayscale(), "…and the render still has to turn grey");
    // The photographer's own switch is still enough on its own.
    let own = EditRecipe { convert_to_grayscale: true, ..Default::default() };
    assert!(own.renders_grayscale(), "either switch, not both");
}

/// A pass-through value is never "unparsable", because it is never parsed.
///
/// The trap this closes: the pass-through keys joined `owned_attr_keys`, and
/// that list IS `unparsable_crs_numbers`' universe — with a ±100 fallback
/// band for any key the registry states no range for. Without the
/// exemption, `crs:UprightFocalLength35mm="104.944461871"` (a real 105 mm
/// prime in this library, well outside ±100) would have been reported as a
/// value that "imports as a silent neutral" — about the one block in the
/// recipe that has no neutral and is never replaced.
///
/// `crs:CameraProfile` is here for the SECOND reason (v1.5.0 F7): it is an
/// owned control now, and a control's exemption comes from its registry
/// SHAPE. A name is not a number whichever list it sits on.
///
/// The sample used to be `crs:PerspectiveX="-140"`; v1.5.0 F6 owns that key
/// with a stated band, so an out-of-band value there IS worth reporting now
/// and the sample moved to a key that is still carried.
#[test]
fn a_passthrough_value_is_never_called_unparsable() {
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                   xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
                   <rdf:Description rdf:about=\"\" \
                   xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
                   crs:CameraProfile=\"Adobe Standard\" \
                   crs:UprightFocalLength35mm=\"104.944461871\" \
                   crs:UprightVersion=\"151388160\" crs:Contrast2012=\"+22\" \
                   crs:HasSettings=\"True\"/></rdf:RDF></x:xmpmeta>";
    assert!(unparsable_crs_numbers(doc).is_empty(), "{:?}", unparsable_crs_numbers(doc));
    // The premise, so the emptiness above is not emptiness for another
    // reason: the same scan still names an OWNED number that is off its
    // band, in the very same document.
    let bad = doc.replace(r#"crs:Contrast2012="+22""#, r#"crs:Contrast2012="+220""#);
    assert_eq!(unparsable_crs_numbers(&bad), vec!["Contrast2012"]);
    // …and the values still arrive, out of band and all.
    let r = xmp_to_recipe(doc);
    assert_eq!(
        r.passthrough.get("UprightFocalLength35mm").map(String::as_str),
        Some("104.944461871")
    );
    assert_eq!(r.passthrough.get("UprightVersion").map(String::as_str), Some("151388160"));
}

/// **The disclosure that had no other half** (R25 B4, work order 4.7):
/// `global_export_losses` has named "we render it, the sidecar cannot
/// carry it" since R24-5 M0; this names the opposite corner — Lightroom
/// renders it, this engine does not — which R25's B2/B3 batches filled
/// with twenty-four members and B4 with the twenty-fifth.
#[test]
fn the_render_gaps_name_what_lightroom_renders_and_this_engine_does_not() {
    use crate::advisor::catalogue::{Tier, RECIPE_CONTROLS};
    // NEUTRAL SAYS NOTHING — and against the DEFAULT, not against zero.
    // The de-fringe block's neutral is Adobe's own 30/70/40/60 (R25 B3),
    // so a zero comparison would report a de-fringe gap on every single
    // photo ever opened, and a disclosure that fires always is read never.
    assert!(
        global_render_gaps(&EditRecipe::default()).is_empty(),
        "a default recipe carries no gap: {:?}",
        global_render_gaps(&EditRecipe::default())
    );
    let untouched = xmp_to_recipe(
        "<rdf:Description xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
             crs:DefringePurpleAmount=\"0\" crs:DefringePurpleHueLo=\"30\" \
             crs:DefringePurpleHueHi=\"70\" crs:DefringeGreenAmount=\"0\" \
             crs:DefringeGreenHueLo=\"40\" crs:DefringeGreenHueHi=\"60\"/>",
    );
    assert!(
        global_render_gaps(&untouched).is_empty(),
        "a real sidecar's resting de-fringe block is not a gap: {:?}",
        global_render_gaps(&untouched)
    );
    // A RENDERED control is not a gap, however far it is from neutral —
    // including the grain, which was this test's example until v1.5.0
    // gave the engine a stage after the crop (`render/finish.rs`).
    let bright = EditRecipe {
        exposure_ev: 2.0,
        texture: 40.0,
        grain: 30.0,
        post_crop_vignette: -40.0,
        ..Default::default()
    };
    assert!(global_render_gaps(&bright).is_empty(), "{:?}", global_render_gaps(&bright));
    // The B4 row is NOT here, and that is the tier's own definition
    // rather than an omission: we never interpret a pass-through value,
    // so we cannot tell Lightroom's resting `PerspectiveUpright="0"` — on
    // six of the seven reference sidecars, changing nothing anywhere —
    // from a real Upright correction. This sentence would then appear on
    // every Lightroom photo ever opened and drown the members that ARE
    // actionable. Its disclosure is the develop panel's own read-only
    // section, which shows the values instead of guessing at them.
    let upright = EditRecipe {
        passthrough: [("PerspectiveVertical".to_string(), "-35".to_string())]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    assert!(
        global_render_gaps(&upright).is_empty(),
        "PassThrough has no knowable neutral, so it discloses through its section"
    );
    assert!(
        RECIPE_CONTROLS
            .iter()
            .any(|c| c.name == "passthrough" && c.tier == Some(Tier::PassThrough)),
        "premise: the row exists and renders nothing — the exclusion above is a choice"
    );
    // **v1.5.0's own sentence**: a recipe with EVERY control away from its
    // neutral still carries no gap, because Track F left nothing carried.
    // Built through serde so a renamed field cannot slip past.
    let mut every = serde_json::to_value(EditRecipe::default()).expect("serialises");
    for c in RECIPE_CONTROLS.iter().filter(|c| c.shape.is_scalar()) {
        every[c.name] = match c.shape {
            crate::advisor::catalogue::Shape::Bool => serde_json::json!(true),
            _ => serde_json::json!(c.range.map_or(7.0, |(_, hi)| hi.min(7.0))),
        };
    }
    let all: EditRecipe = serde_json::from_value(every).expect("in range");
    assert!(
        global_render_gaps(&all).is_empty(),
        "nothing is carried any more, so nothing can be a gap: {:?}",
        global_render_gaps(&all)
    );
    assert!(
        RECIPE_CONTROLS.iter().any(|c| c.name == "defringe_purple" && c.tier == Some(Tier::Rendered)),
        "premise: the de-fringe block was the LAST carried row, and it renders now — \
             without this the emptiness above would prove nothing about the batch"
    );
    // THE MECHANISM, which the real registry can no longer exercise: one
    // synthetic CarriedOnly row, over a real serde field so the neutral
    // comparison has something to read. This is the disclosure the day a
    // carried control comes back — and the reason `render_gaps_in` is a
    // function of its registry rather than a closure over the global one.
    let carried = crate::advisor::catalogue::Control {
        name: "defringe_purple",
        shape: crate::advisor::catalogue::Shape::Number,
        range: Some((0.0, 20.0)),
        neutral: "0",
        engine_only: true,
        crs: crate::advisor::catalogue::CrsKey::Attr("DefringePurpleAmount"),
        tier: Some(Tier::CarriedOnly),
        purpose: "a synthetic carried row: the disclosure has to work before it is needed",
    };
    let fringed = EditRecipe { defringe_purple: 3.0, ..Default::default() };
    assert_eq!(
        render_gaps_in(std::slice::from_ref(&carried), &fringed),
        vec!["defringe_purple"],
        "a carried control away from its neutral is named"
    );
    assert!(
        render_gaps_in(std::slice::from_ref(&carried), &EditRecipe::default()).is_empty(),
        "…and at its neutral it is not"
    );
    // The two disclosures are DISJOINT halves of one story, never the same
    // claim twice: a tier renders or it does not. Trivially satisfied while
    // the left half is empty — asserted anyway, because the day it is not
    // empty is the day this matters and nobody will think to add it then.
    for g in render_gaps_in(std::slice::from_ref(&carried), &fringed) {
        assert!(
            !global_export_losses(&fringed).contains(&g),
            "{g} cannot be both a render gap and an export loss"
        );
    }
    let _ = Tier::PassThrough; // the tier this batch populated
}

/// R25 B2 (policy SF4-C): the nine carried effects reach the sidecar and
/// the engine renders nothing from them.
///
/// The write rule is PER-KEY — every one of the nine is neutral at zero,
/// so "write what is non-neutral" needs no group gate, and the three whose
/// ACR default is not zero (Midpoint/Feather 50, Style 1) reach Lightroom
/// by ABSENCE rather than by a value we made up. Verified against the
/// user's own library, where Lightroom writes the companion keys only
/// alongside a non-zero amount.
#[test]
fn carried_effects_round_trip_and_render_nothing() {
    // The shape a real Lightroom sidecar takes (P34 / P05):
    // an amount plus its five companions, grain likewise.
    let r = EditRecipe {
        post_crop_vignette: -17.0,
        post_crop_vignette_mid: 50.0,
        post_crop_vignette_feather: 50.0,
        post_crop_vignette_round: 0.0,
        post_crop_vignette_style: 1.0,
        post_crop_vignette_hl: 0.0,
        grain: 30.0,
        grain_size: 25.0,
        grain_rough: 50.0,
        ..Default::default()
    };
    let xmp = recipe_to_xmp(&r);
    for want in [
        r#"crs:PostCropVignetteAmount="-17""#,
        r#"crs:PostCropVignetteMidpoint="50""#,
        r#"crs:PostCropVignetteFeather="50""#,
        r#"crs:PostCropVignetteStyle="1""#,
        r#"crs:GrainAmount="30""#,
        r#"crs:GrainSize="25""#,
        r#"crs:GrainFrequency="50""#,
    ] {
        assert!(xmp.contains(want), "{want} missing from: {xmp}");
    }
    // The two that are AT their neutral stay out — absence is how
    // Lightroom is told "keep your own default".
    assert!(!xmp.contains("PostCropVignetteRoundness"), "a zero roundness is not written");
    assert!(!xmp.contains("PostCropVignetteHighlightContrast"));
    // Every one comes back as itself — read side and write side in one
    // batch, exactly as for Texture above.
    let back = xmp_to_recipe(&xmp);
    for (name, live, want) in [
        ("post_crop_vignette", back.post_crop_vignette, r.post_crop_vignette),
        ("post_crop_vignette_mid", back.post_crop_vignette_mid, r.post_crop_vignette_mid),
        (
            "post_crop_vignette_feather",
            back.post_crop_vignette_feather,
            r.post_crop_vignette_feather,
        ),
        ("post_crop_vignette_round", back.post_crop_vignette_round, r.post_crop_vignette_round),
        ("post_crop_vignette_style", back.post_crop_vignette_style, r.post_crop_vignette_style),
        ("post_crop_vignette_hl", back.post_crop_vignette_hl, r.post_crop_vignette_hl),
        ("grain", back.grain, r.grain),
        ("grain_size", back.grain_size, r.grain_size),
        ("grain_rough", back.grain_rough, r.grain_rough),
    ] {
        assert_eq!(live, want, "{name} did not survive the round trip");
    }
    // A neutral recipe emits none of the nine (byte-compatible with the
    // pre-B2 writer for every recipe that never touched them).
    let neutral = recipe_to_xmp(&EditRecipe::default());
    for k in ["PostCropVignette", "Grain"] {
        assert!(!neutral.contains(k), "{k} must not appear on a neutral recipe: {neutral}");
    }
    // …and the ENGINE ignores all nine: the developed frame is
    // bit-identical to the neutral one. That is the claim
    // `Tier::CarriedOnly` makes, and it is the half a registry row cannot
    // prove about itself.
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(24, 16, |x, y| {
        image::Rgb([(x * 9) as u8, (y * 13) as u8, (x + y) as u8])
    }));
    assert_eq!(
        crate::render::develop_preview(&img, &r).to_rgb8().into_raw(),
        crate::render::develop_preview(&img, &EditRecipe::default()).to_rgb8().into_raw(),
        "a CarriedOnly control that moved a pixel would be mis-classified"
    );
}

/// One parametric mask + one raster mask — the fixture behind BOTH halves
/// of the raster contract: the writer emits no raster correction, and the
/// reader therefore returns no phantom for one.
fn mixed_parametric_and_raster() -> EditRecipe {
    EditRecipe {
        masks: vec![
            LocalAdjustment {
                mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.35, full_x: 0.5, full_y: 0.0 },
                exposure_ev: -1.0,
                ..Default::default()
            },
            LocalAdjustment {
                mask: MaskGeometry::Bitmap { path: "out/subject.png".into() },
                exposure_ev: 0.6,
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

#[test]
fn bitmap_masks_are_skipped_by_the_xmp_writer() {
    use crate::recipe::MaskGeometry;
    let mixed = mixed_parametric_and_raster();
    let xmp = recipe_to_xmp(&mixed);
    assert!(xmp.contains("Mask/Gradient"), "the parametric mask must survive");
    assert_eq!(xmp.matches("crs:What=\"Correction\"").count(), 1, "raster correction skipped");
    assert!(!xmp.contains("subject.png"), "no raster path may leak into the sidecar");
    // All-raster: the whole corrections block disappears (no empty shell).
    let all_bitmap = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Bitmap { path: "out/sky.png".into() },
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(!recipe_to_xmp(&all_bitmap).contains("MaskGroupBasedCorrections"));
}

/// M6a: the export direction had FOUR silent losses (raster masks skipped,
/// muted masks skipped, Bitmap components omitted, radial rotation +
/// recolour gains dropped) against four import-side disclosures and zero
/// export-side ones. The writer now names them while it emits, and the
/// assertion is a SET comparison, not a count: a rule that fires on the
/// wrong mask, or twice on one mask, is exactly the bug a count hides.
#[test]
fn the_writer_names_bitmap_components_and_exports_native_components() {
    use crate::recipe::{MaskCombine, MaskComponent};
    let radial = |angle: f32| MaskGeometry::Radial {
        top: 0.3,
        left: 0.35,
        bottom: 0.7,
        right: 0.65,
        feather: 0.5,
        roundness: 0.0,
        flipped: false,
        angle,
        midpoint: 50.0,
        mask_version: 2,
    };
    let component = MaskComponent {
        inverted: false,
        geometry: MaskGeometry::Linear { zero_x: 0.1, zero_y: 0.1, full_x: 0.9, full_y: 0.9 },
        mode: MaskCombine::Subtract,
    };
    let r = EditRecipe {
        masks: vec![
            // Raster geometry: the whole correction goes.
            LocalAdjustment {
                mask: MaskGeometry::Bitmap { path: "out/sky.png".into() },
                name: "sky".into(),
                ..Default::default()
            },
            // Muted — and loaded with every degradation as well: the eye
            // is why it is skipped, so it must produce exactly ONE verdict
            // (this arm is what stops the counts from double-billing).
            LocalAdjustment {
                mask: radial(30.0),
                components: vec![component.clone()],
                color_gains: Some([1.4, 1.0, 0.6]),
                enabled: false,
                name: "parked".into(),
                ..Default::default()
            },
            // Base plus native component emit; only the Bitmap extra is lost.
            LocalAdjustment {
                components: vec![component.clone(), MaskComponent {
                    geometry: MaskGeometry::Bitmap { path: "out/component.png".into() },
                    mode: MaskCombine::Intersect, inverted: false,
                }],
                name: "combo".into(),
                ..Default::default()
            },
            // Emitted as an UNROTATED ellipse, without the recolour.
            LocalAdjustment {
                mask: radial(-12.0),
                color_gains: Some([1.2, 0.95, 0.7]),
                name: "gold".into(),
                ..Default::default()
            },
            // Nothing lost: an unrotated radial, no components, neutral
            // gains (which are no recolour at all).
            LocalAdjustment {
                mask: radial(0.0),
                color_gains: Some([1.0, 1.0, 1.0]),
                name: "clean".into(),
                ..Default::default()
            },
            // Unnamed masks are identified by the name the SIDECAR would
            // have used, not by "".
            LocalAdjustment { mask: MaskGeometry::Bitmap { path: "out/x.png".into() }, ..Default::default() },
        ],
        ..Default::default()
    };
    let mut got: Vec<(String, MaskLossReason)> =
        mask_export_losses(&r).into_iter().map(|l| (l.name, l.reason)).collect();
    got.sort();
    let mut want = vec![
        ("sky".to_string(), MaskLossReason::Bitmap),
        ("parked".to_string(), MaskLossReason::Disabled),
        ("combo".to_string(), MaskLossReason::ComponentsFlattened),
        // …and the rotation verdict CARRIES the angle it dropped (R25
        // P5): −12°, not merely "some rotation". The disclosure surfaces
        // read the number off this payload, so a writer that raised the
        // reason with the wrong angle fails HERE, at the source, rather
        // than printing a plausible wrong number in the window.
        ("gold".to_string(), MaskLossReason::Rotation(-12)),
        ("gold".to_string(), MaskLossReason::Recolour),
        ("AutoShade 6".to_string(), MaskLossReason::Bitmap),
    ];
    want.sort();
    assert_eq!(got, want, "the loss set must name mask AND reason exactly once each");
    // The prose channel (CLI stderr / web reply) covers every category and
    // counts them; "combo" drops only its one Bitmap component.
    let line = describe_mask_losses(&mask_export_losses(&r)).expect("losses ⇒ a line");
    for expect in [
        "2 bitmap mask(s) skipped (sky, AutoShade 6)",
        "1 muted mask(s) skipped (parked)",
        "1 bitmap component(s) omitted (combo)",
        "1 radial rotation dropped (gold)",
        "1 recolour gains dropped (gold)",
    ] {
        assert!(line.contains(expect), "the line must state {expect:?}: {line}");
    }
    // …and the emitted document agrees with the claim: four masks, one
    // skipped as raster, one as muted, and no leaked raster path.
    let doc = recipe_to_xmp(&r);
    assert_eq!(doc.matches("crs:What=\"Correction\"").count(), 3, "3 of 6 project");
    assert!(!doc.contains("sky.png"), "no raster path in a sidecar");

    // Nothing lossy ⇒ nothing said, on both channels (a faithful save
    // must not be interrupted).
    let faithful = EditRecipe { masks: vec![r.masks[4].clone()], ..Default::default() };
    assert!(mask_export_losses(&faithful).is_empty(), "an exportable mask loses nothing");
    assert!(describe_mask_losses(&[]).is_none(), "an empty list has nothing to say");
    assert!(mask_export_losses(&EditRecipe::default()).is_empty(), "no masks, no losses");
}

/// R25 P0-0.6: both disclosure surfaces ITERATE `MaskLossReason::ALL`
/// (here and the GUI's `xmp_loss_line`), so the list is the one place a
/// reason can be forgotten — and the match below is where a new variant
/// stops the build, with `ALL` the next thing it has to satisfy.
#[test]
fn mask_loss_reason_all_covers_every_variant() {
    // Adding a variant makes THIS match non-exhaustive; the arm you write
    // carries the next rank, and the two asserts then fail until `ALL`
    // lists the newcomer in that position.
    fn rank(r: MaskLossReason) -> usize {
        match r {
            MaskLossReason::Bitmap => 0,
            MaskLossReason::Disabled => 1,
            MaskLossReason::ComponentsFlattened => 2,
            MaskLossReason::BrushRendered => 3,
            MaskLossReason::AiMaskRecomputed => 4,
            MaskLossReason::Rotation(_) => 5,
            MaskLossReason::Recolour => 6,
            MaskLossReason::RasterNotEmbedded => 7,
        }
    }
    for (i, r) in MaskLossReason::ALL.into_iter().enumerate() {
        assert_eq!(rank(r), i, "ALL must list every reason once, in rank order");
        assert!(!r.en().trim().is_empty(), "{r:?} has no label for the prose channel");
        assert!(r.same_kind(r), "same_kind must be reflexive for {r:?}");
    }
    // R25 P5: `Rotation` grew a payload, so the grouping key is the
    // DISCRIMINANT — two masks tilted differently are one line in a
    // sentence and two values under `==`, and `ALL`'s placeholder `0`
    // equals neither of them. Same property the import twin relies on.
    assert!(
        MaskLossReason::Rotation(37).same_kind(MaskLossReason::Rotation(-12)),
        "two tilted masks are one line"
    );
    assert!(
        !MaskLossReason::Rotation(0).same_kind(MaskLossReason::Recolour),
        "different variants are different lines"
    );
    // Every reason the WRITER can raise reaches the prose. The mutation
    // this catches: a sixth reason raised by `masks_xml` and left out of
    // `ALL` would be silently invisible in the sentence.
    let losses: Vec<MaskLoss> = MaskLossReason::ALL
        .into_iter()
        .map(|reason| MaskLoss { name: format!("m{}", rank(reason)), reason })
        .collect();
    let line = describe_mask_losses(&losses).expect("five losses ⇒ a line");
    for r in MaskLossReason::ALL {
        assert!(line.contains(r.en()), "{r:?} never reaches the prose: {line}");
        assert!(line.contains(&format!("m{}", rank(r))), "{r:?} loses its mask name: {line}");
    }
}

// ── R25 P1: the import unlock ────────────────────────────────────────
//
// FIXTURE POLICY. This is a public repository and the reference sidecars
// are the user's own photographs, so no line of them is copied in. Every
// inline fixture below is SYNTHESISED: the attribute set, the value
// shapes, the attribute order and the element nesting are reproduced
// exactly as `P50.xmp` / `P51.xmp` write them, and every
// personal identifier (`crs:CorrectionName`, `crs:MaskName`, the sync
// GUIDs) carries a neutral test value instead. The real files are
// exercised by `real_lightroom_sidecars_import_their_parametric_masks`,
// which reads them from a path given at RUN time and skips when it is
// not set.

/// One `crs:What="Correction"` `<rdf:li>`, structurally verbatim: all 25
/// `crs:Local*` attributes in Lightroom's own order, the sliders on its
/// own 0..1 scale, `crs:CorrectionMasks` last. `locals` is spliced in
/// just before `LocalCurveRefineSaturation` (where an unknown key would
/// really sit); tests that need a DIFFERENT value for an existing key
/// rewrite it on the returned string, so the fixture can never carry the
/// same attribute twice.
fn lr_correction(name: &str, locals: &str, components: &str) -> String {
    lr_correction_with_curves(name, locals, "", components)
}

/// The same fixture with the correction's four LOCAL point curves spliced
/// in (R25 P6) — child ELEMENTS between the attribute block's closing `>`
/// and `<crs:CorrectionMasks>`, which is where the reference sidecars put
/// them. `lr_curve` builds one.
fn lr_correction_with_curves(
    name: &str,
    locals: &str,
    curves: &str,
    components: &str,
) -> String {
    format!(
        "     <rdf:li>\n\
             \x20     <rdf:Description\n\
             \x20      crs:What=\"Correction\"\n\
             \x20      crs:CorrectionAmount=\"1\"\n\
             \x20      crs:CorrectionActive=\"true\"\n\
             \x20      crs:CorrectionName=\"{name}\"\n\
             \x20      crs:CorrectionSyncID=\"0000000000000000000000000000000A\"\n\
             \x20      crs:LocalExposure=\"0\"\n\
             \x20      crs:LocalHue=\"0\"\n\
             \x20      crs:LocalSaturation=\"0\"\n\
             \x20      crs:LocalContrast=\"0\"\n\
             \x20      crs:LocalClarity=\"0\"\n\
             \x20      crs:LocalSharpness=\"0\"\n\
             \x20      crs:LocalBrightness=\"0\"\n\
             \x20      crs:LocalToningHue=\"0\"\n\
             \x20      crs:LocalToningSaturation=\"0\"\n\
             \x20      crs:LocalExposure2012=\"0.1\"\n\
             \x20      crs:LocalContrast2012=\"0.43\"\n\
             \x20      crs:LocalHighlights2012=\"0\"\n\
             \x20      crs:LocalShadows2012=\"0\"\n\
             \x20      crs:LocalWhites2012=\"0\"\n\
             \x20      crs:LocalBlacks2012=\"0\"\n\
             \x20      crs:LocalClarity2012=\"0\"\n\
             \x20      crs:LocalDehaze=\"0\"\n\
             \x20      crs:LocalLuminanceNoise=\"0\"\n\
             \x20      crs:LocalMoire=\"0\"\n\
             \x20      crs:LocalDefringe=\"0\"\n\
             \x20      crs:LocalTemperature=\"0.24\"\n\
             \x20      crs:LocalTint=\"0.44\"\n\
             \x20      crs:LocalTexture=\"0\"\n\
             \x20      crs:LocalGrain=\"0\"\n\
             {locals}\
             \x20      crs:LocalCurveRefineSaturation=\"100\">\n\
             {curves}\
             \x20     <crs:CorrectionMasks>\n\
             \x20      <rdf:Seq>\n\
             {components}\
             \x20      </rdf:Seq>\n\
             \x20     </crs:CorrectionMasks>\n\
             \x20     </rdf:Description>\n\
             \x20    </rdf:li>\n"
    )
}

/// One local point curve as Lightroom writes it inside a Correction: a
/// BARE key (`MainCurve`, not `ToneCurvePV2012`) and points spelled `x,y`
/// with NO space after the comma — structurally verbatim from
/// `P51.xmp`'s `<crs:RedCurve>` block, with test values.
fn lr_curve(tag: &str, points: &[(u8, u8)]) -> String {
    let pts: String = points
        .iter()
        .map(|(x, y)| format!("       <rdf:li>{x},{y}</rdf:li>\n"))
        .collect();
    format!(
        "      <crs:{tag}>\n       <rdf:Seq>\n{pts}       </rdf:Seq>\n      </crs:{tag}>\n"
    )
}

/// A radial component the way Lightroom writes one — `crs:Angle` and
/// `crs:MaskBlendMode` included, because it writes them on EVERY radial.
fn lr_radial(angle: &str, blend: &str) -> String {
    format!(
        "        <rdf:li\n\
             \x20        crs:What=\"Mask/CircularGradient\"\n\
             \x20        crs:MaskActive=\"true\"\n\
             \x20        crs:MaskName=\"Radial Gradient 1\"\n\
             \x20        crs:MaskBlendMode=\"{blend}\"\n\
             \x20        crs:MaskInverted=\"false\"\n\
             \x20        crs:MaskSyncID=\"0000000000000000000000000000000B\"\n\
             \x20        crs:MaskValue=\"1\"\n\
             \x20        crs:Top=\"0.114928\"\n\
             \x20        crs:Left=\"0.590368\"\n\
             \x20        crs:Bottom=\"0.802847\"\n\
             \x20        crs:Right=\"0.921381\"\n\
             \x20        crs:Angle=\"{angle}\"\n\
             \x20        crs:Midpoint=\"50\"\n\
             \x20        crs:Roundness=\"0\"\n\
             \x20        crs:Feather=\"100\"\n\
             \x20        crs:Flipped=\"true\"\n\
             \x20        crs:Version=\"2\"/>\n"
    )
}

/// A linear gradient component, same provenance.
fn lr_gradient(blend: &str) -> String {
    format!(
        "        <rdf:li\n\
             \x20        crs:What=\"Mask/Gradient\"\n\
             \x20        crs:MaskActive=\"true\"\n\
             \x20        crs:MaskName=\"Linear Gradient 1\"\n\
             \x20        crs:MaskBlendMode=\"{blend}\"\n\
             \x20        crs:MaskInverted=\"false\"\n\
             \x20        crs:MaskSyncID=\"0000000000000000000000000000000C\"\n\
             \x20        crs:MaskValue=\"1\"\n\
             \x20        crs:ZeroX=\"0.5\"\n\
             \x20        crs:ZeroY=\"0.8\"\n\
             \x20        crs:FullX=\"0.5\"\n\
             \x20        crs:FullY=\"0.2\"/>\n"
    )
}

/// The surrounding document: a Lightroom catalog export, NOT one of ours
/// (`x:xmptk` is Adobe's), which is the whole point — every gate this
/// batch reopened keyed on our own provenance.
fn lr_doc(corrections: &str) -> String {
    format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Adobe XMP Core 5.6-c145\">\n\
             \x20<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
             \x20 <rdf:Description rdf:about=\"\"\n\
             \x20   xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n\
             \x20   crs:Version=\"15.5.1\"\n\
             \x20   crs:ProcessVersion=\"15.4\"\n\
             \x20   crs:Exposure2012=\"+0.35\"\n\
             \x20   crs:HasSettings=\"True\">\n\
             \x20  <crs:MaskGroupBasedCorrections>\n\
             \x20   <rdf:Seq>\n\
             {corrections}\
             \x20   </rdf:Seq>\n\
             \x20  </crs:MaskGroupBasedCorrections>\n\
             \x20 </rdf:Description>\n\
             \x20</rdf:RDF>\n\
             </x:xmpmeta>\n"
    )
}

/// §0 OF THE ROUND. Lightroom writes `crs:Angle` on every radial — `"0"`
/// when the shape was never rotated — and the import refused the WHOLE
/// correction on its mere presence. Every radial mask in the user's
/// catalog therefore arrived as nothing, and the only thing said about it
/// was an integer count.
///
/// MUTATION THIS CATCHES: put `tag.crs_str("Angle").is_some()` back into
/// the refusal and this test goes to zero masks — which is exactly the
/// state the round opened in.
#[test]
fn a_lightroom_radial_with_angle_imports() {
    let doc = lr_doc(&lr_correction("Radial 1", "", &lr_radial("37.412506", "0")));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "a rotated Lightroom radial must import: {:?}", r.masks);
    // The angle itself is NOT mapped here — not because the sign or pivot
    // are unknown (v0.32.0 measured both), but because `lr_doc` declares
    // no frame, which is the one case left. The mask arrives as its
    // axis-aligned ellipse, and the note says so.
    let MaskGeometry::Radial { angle, top, feather, flipped, .. } = r.masks[0].mask else {
        panic!("expected a radial, got {:?}", r.masks[0].mask);
    };
    assert_eq!(angle, 0.0, "crs:Angle is disclosed, not guessed at");
    // v0.32.0: `lr_doc` declares no `tiff:ImageWidth/ImageLength`, so the
    // pixel→normalised fold has no aspect and the tilt is disclosed rather
    // than applied (the assert above, and the `Rotation` note below). The
    // FRAME AFFINE is applied to every radial — and since the 2026-08-19
    // ruling set `LR_MASK_FRAME_SCALE = 1.0` (Batch-10: the sidecar's
    // geometry lives in the PLAIN frame; the old 1.032 was one frame's
    // lens-profile warp), the affine is the identity and the corner is
    // the STORED number. Hand-checked: `cy = 0.4588875`,
    // `ry = 0.3439595`, top = cy − ry = 0.1149280.
    assert!(
        (top as f64 - 0.1149280).abs() < 1e-7,
        "the geometry is the file's, in the plain frame: {top}"
    );
    assert_eq!(feather, 1.0, "crs:Feather=100 is Lightroom's 0..100 scale");
    // R25 P9: `crs:Flipped="true"` beside `crs:MaskInverted="false"` is
    // Lightroom's NOT-inverted spelling — one bit written twice — so the
    // mask must arrive with neither flag set. It used to arrive flipped,
    // which inverted it.
    assert!(!flipped, "crs:Flipped is not a second inversion flag");
    assert!(!r.masks[0].inverted, "and MaskInverted=false is the file's actual verdict");
    assert_eq!(r.masks[0].exposure_ev, 0.4, "0.1 × 4 stops");
    assert_eq!(unsupported_corrections(&doc), 0, "nothing was refused");
    let losses = import_losses(&doc);
    assert_eq!(
        losses,
        vec![MaskImportLoss {
            name: "Radial 1".into(),
            // R25 P5: the verdict carries the sidecar's own angle, rounded
            // to whole degrees for the sentence that prints it. 37.412506
            // → 37; the recipe keeps nothing of it at all (the ellipse
            // imports axis-aligned), which is exactly why the disclosure
            // has to be able to say how much was set aside.
            reason: MaskImportReason::Rotation(37)
        }],
        "the rotation is named, with its angle, and it is the ONLY loss"
    );
}

// ── R25 P5: the geometry write-back (B5-B1) ──────────────────────────
//
// Same fixture policy as the block above — `lr_radial` reproduces
// Lightroom's own attribute set, order and value shapes for a radial
// component, with neutral identifiers. The two numbers that had to be
// REAL to be worth pinning (the out-of-frame corners) are the measured
// ones from the reference library.

/// `crs:Midpoint` and `crs:Version` sit on EVERY Lightroom radial, and
/// until this batch on neither side of this engine: not read, therefore
/// not kept, therefore deleted from the photographer's own sidecar the
/// first time AutoShade rewrote it. Both directions in one test, because
/// a read without a write is the worse half — it looks like it works.
///
/// The values are deliberately NON-default (37 / 3): 50 / 2 are what a
/// reader that ignores both attributes also produces.
///
/// MUTATION THIS CATCHES: default either field on the way in, or drop
/// either attribute on the way out, and 37 / 3 vanish.
#[test]
fn radial_midpoint_and_version_round_trip() {
    let comp = lr_radial("0", "0")
        .replace("crs:Midpoint=\"50\"", "crs:Midpoint=\"37\"")
        .replace("crs:Version=\"2\"", "crs:Version=\"3\"");
    let doc = lr_doc(&lr_correction("Radial 1", "", &comp));
    let r = xmp_to_recipe(&doc);
    let MaskGeometry::Radial { midpoint, mask_version, .. } = r.masks[0].mask else {
        panic!("expected a radial, got {:?}", r.masks[0].mask);
    };
    assert_eq!(midpoint, 37.0, "crs:Midpoint is read, not defaulted");
    assert_eq!(mask_version, 3, "crs:Version is read, not defaulted");
    assert!(import_losses(&doc).is_empty(), "carrying a value is not losing it");

    // …and out again, adjacent and in the writer's own order.
    let out = recipe_to_xmp(&r);
    assert!(
        out.contains("crs:Midpoint=\"37\" crs:Version=\"3\""),
        "both ride back out of the writer: {out}"
    );
    assert_eq!(
        xmp_to_recipe(&out).masks[0].mask,
        r.masks[0].mask,
        "our own document re-reads to the same geometry"
    );

    // The collision the bounded read exists for: `range_mask_xml` writes
    // `crs:Version="3"` on the RANGE component, which sits after the
    // geometry inside the same correction. An unbounded scan reads that
    // as the ellipse's schema stamp and quietly promotes every ranged
    // radial from 2 to 3.
    let ranged = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Radial {
                top: 0.3, left: 0.35, bottom: 0.7, right: 0.65,
                feather: 0.5, roundness: 0.0, flipped: false, angle: 0.0,
                midpoint: 50.0, mask_version: 2,
            },
            range: Some(RangeMask::Luminance {
                lo_outer: 0.1,
                lo: 0.2,
                hi: 0.8,
                hi_outer: 0.9,
            }),
            name: "ranged".into(),
            exposure_ev: 0.5,
            ..Default::default()
        }],
        ..Default::default()
    };
    let back = xmp_to_recipe(&recipe_to_xmp(&ranged));
    let MaskGeometry::Radial { mask_version, .. } = back.masks[0].mask else {
        panic!("expected a radial, got {:?}", back.masks[0].mask);
    };
    assert_eq!(mask_version, 2, "the range component's own Version is not the ellipse's");
}

/// The reference library holds radial corners on BOTH sides of the frame
/// — `crs:Bottom="1.802847"` in one file, `crs:Top="-0.153271"` in
/// another. ACR geometry is a centre+radii carrier, so this is ordinary,
/// not corruption: the reader already widened to ±8, and the WRITER must
/// not quietly pull them back to 0..1 on the way out (a clamp there
/// shortens the falloff of every off-frame gradient the user placed).
#[test]
fn out_of_frame_radial_corners_survive_a_round_trip() {
    let comp = lr_radial("0", "0")
        .replace("crs:Top=\"0.114928\"", "crs:Top=\"-0.153271\"")
        .replace("crs:Bottom=\"0.802847\"", "crs:Bottom=\"1.802847\"");
    let doc = lr_doc(&lr_correction("Radial 1", "", &comp));
    let r = xmp_to_recipe(&doc);
    let MaskGeometry::Radial { top, bottom, .. } = r.masks[0].mask else {
        panic!("expected a radial, got {:?}", r.masks[0].mask);
    };
    // The corners arrive VERBATIM (the frame affine is the identity since
    // the 2026-08-19 `LR_MASK_FRAME_SCALE = 1.0` ruling) — the point of
    // the test is that nothing pulls an off-frame corner back to 0..1,
    // and that holds on both boundaries.
    assert!(
        (top as f64 - -0.153271).abs() < 1e-7,
        "a corner above the frame is a real value: {top}"
    );
    assert!(
        (bottom as f64 - 1.802847).abs() < 1e-7,
        "and so is one below it: {bottom}"
    );
    let out = recipe_to_xmp(&r);
    // …and the WRITER hands the file its own numbers back, to the byte.
    assert!(out.contains("crs:Top=\"-0.153271\""), "written raw, not clamped: {out}");
    assert!(out.contains("crs:Bottom=\"1.802847\""), "written raw, not clamped: {out}");
    assert_eq!(xmp_to_recipe(&out).masks[0].mask, r.masks[0].mask, "and it is stable");
}

// ── v0.32.0: the radial geometry projection ─────────────────────────────
//
// The fixtures below are the SIDECAR NUMBERS of the user's own twelve-frame
// controlled Lightroom experiment, transcribed from
// `lr-experiment/probe2/probe2-extract.txt` in the R25 materials ledger, outside the tree
// and `.../lr-experiment/extract.txt`. No photograph and no export is in
// the repository — the RENDERED measurements those exports produced are
// quoted in the assertions, and the fixtures that reproduce them from the
// real files live behind `AUTOSHADE_LR_PROBE_FIXTURES` (see
// `the_probe_sidecars_decode_to_their_measured_ellipses`).

/// A radial component with arbitrary corners, otherwise byte-shaped like
/// [`lr_radial`] (which is transcribed from a real Lightroom write).
fn lr_radial_at(t: &str, l: &str, b: &str, r: &str, angle: &str) -> String {
    lr_radial(angle, "0")
        .replace("crs:Top=\"0.114928\"", &format!("crs:Top=\"{t}\""))
        .replace("crs:Left=\"0.590368\"", &format!("crs:Left=\"{l}\""))
        .replace("crs:Bottom=\"0.802847\"", &format!("crs:Bottom=\"{b}\""))
        .replace("crs:Right=\"0.921381\"", &format!("crs:Right=\"{r}\""))
}

/// Declare the frame on a document that had none. [`lr_doc`] deliberately
/// does NOT — that keeps every older test on the no-frame arm, which is a
/// real arm and has to stay covered — so the tests that need the aspect
/// add it here. Every real Lightroom sidecar carries these two
/// (`P19.xmp`: `tiff:ImageWidth="9504" tiff:ImageLength="6336"`,
/// which is the ARW's own `DefaultCropSize`).
fn in_frame(doc: &str, w: u32, h: u32) -> String {
    doc.replace(
        "crs:Version=\"15.5.1\"",
        &format!("tiff:ImageWidth=\"{w}\"\n   tiff:ImageLength=\"{h}\"\n   crs:Version=\"15.5.1\""),
    )
}

/// The engine's stored radial re-expressed as the PIXEL ellipse it draws:
/// `(semi-axis along the tilt, the other one, tilt in degrees)`, both axes
/// in pixels of a `w × h` frame. This is the quantity the probes measured,
/// so it is the quantity the assertions can quote.
fn engine_pixel_ellipse(m: &MaskGeometry, w: f64, h: f64) -> (f64, f64, f64) {
    let MaskGeometry::Radial { top, left, bottom, right, angle, .. } = m else {
        panic!("expected a radial, got {m:?}");
    };
    let (rx, ry) = (
        ((*right as f64 - *left as f64) / 2.0).abs() * w,
        ((*bottom as f64 - *top as f64) / 2.0).abs() * h,
    );
    // The engine rotates in the NORMALISED frame, so the pixel ellipse is
    // `diag(w, h)·R(angle)·diag(rx/w, ry/h)` — written out with the `w`/`h`
    // already folded into `rx`/`ry` above.
    let (sin, cos) = (*angle as f64).to_radians().sin_cos();
    let (s1, s2, tu) = svd2([cos * rx, -sin * ry * w / h, sin * rx * h / w, cos * ry]);
    (s1.abs(), s2.abs(), tu.to_degrees())
}

/// §0 OF v0.32.0. `crs:Top/Left/Bottom/Right` are the ROTATED CORNERS of
/// the ellipse's box in pixel space, and reading them as a bounding box —
/// which every build up to v0.31.2 did — gets the SHAPE wrong by factors,
/// not percentages.
///
/// Both rotated probes are here, and they are the two that discriminate:
///
/// | probe | file | `crs:Angle` | naive `a/b` | corner `a/b` | measured |
/// |---|---|---|---|---|---|
/// | #4 | `P24` | +24.348422 | **1.652** | **8.332** | 6.3–9.8, scan optimum 7.92 |
/// | #8 | `P22` | +29.513785 | **−0.032** (impossible) | **0.524** | 1.84–2.09, scan optimum 1.907 |
///
/// and `#8`'s decoded MAJOR axis is `b`, so its predicted screen tilt is
/// `θ + 90 = −60.486°` against a measured **−60.5° ± 0.9** (mean over the
/// τ ≥ 0.8 level sets). `PROBE2-VERDICT.md` §1, §5.
///
/// MUTATION THIS CATCHES: take `abs()` on `X`/`Y` in `lr_to_engine` and
/// `#8` — whose `Left > Right` — decodes to the naive sliver again; drop
/// the `sin`/`cos` mixing and both ratios collapse onto the naive column.
#[test]
fn a_rotated_lightroom_radial_decodes_to_the_measured_ellipse() {
    let (w, h) = (9504.0, 6336.0);
    // probe #4 — `P24`, an 8:1 sliver rotated +24.35°.
    let doc = in_frame(
        &lr_doc(&lr_correction(
            "R",
            "",
            &lr_radial_at("-0.045582", "-0.056408", "0.9708", "1.062771", "24.348422"),
        )),
        9504,
        6336,
    );
    let m = &xmp_to_recipe(&doc).masks[0].mask;
    // The RATIO is the model's zero-free-parameter prediction and carries
    // no `k`; the axes themselves are the decode × the frame scale, so they
    // are quoted against `k · a` and `k · b`.
    let k = LR_MASK_FRAME_SCALE;
    let (a, b, tilt) = engine_pixel_ellipse(m, w, h);
    assert!((a - k * 6172.8).abs() < 0.5, "semi-major {a} px, decoded 6172.8 × k");
    assert!((b - k * 740.8).abs() < 0.5, "semi-minor {b} px, decoded 740.8 × k");
    assert!((a / b - 8.332).abs() < 0.01, "axis ratio {} — the naive read is 1.652", a / b);
    assert!((tilt - 24.348422).abs() < 1e-3, "screen tilt {tilt}°, declared +24.348422");

    // probe #8 — `P22`, TALL (1:2) and rotated past the inversion
    // point, so Lightroom wrote `Left > Right`. The naive read makes
    // `X = −122 px` and the shape a sliver on the wrong axis.
    let doc = in_frame(
        &lr_doc(&lr_correction(
            "R",
            "",
            &lr_radial_at("-0.088191", "0.520214", "1.113528", "0.494492", "29.513785"),
        )),
        9504,
        6336,
    );
    let m = &xmp_to_recipe(&doc).masks[0].mask;
    let (a, b, tilt) = engine_pixel_ellipse(m, w, h);
    // The SVD reports the MAJOR axis first, and here that is the decoded
    // `b = 3373.2` at `θ + 90`.
    assert!((a - k * 3373.2).abs() < 0.5, "semi-major {a} px, decoded 3373.2 × k");
    assert!((b - k * 1769.1).abs() < 0.5, "semi-minor {b} px, decoded 1769.1 × k");
    assert!((b / a - 0.524).abs() < 0.001, "axis ratio {} — the naive read is −0.032", b / a);
    assert!((tilt - -60.486).abs() < 1e-2, "major-axis tilt {tilt}°, measured −60.5 ± 0.9");
}

/// One mask, one verdict: a mask that names the same missing raster
/// through its base geometry AND a component is one mask that lost one
/// raster, not two losses with the same name.
#[test]
fn a_mask_naming_one_missing_raster_twice_loses_it_once() {
    use crate::recipe::{MaskCombine, MaskComponent};
    let recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Bitmap { path: "gone-raster.png".into() },
            components: vec![MaskComponent {
                geometry: MaskGeometry::Bitmap { path: "gone-raster.png".into() },
                mode: MaskCombine::Add,
                inverted: false,
            }],
            exposure_ev: 0.3,
            ..Default::default()
        }],
        ..Default::default()
    };
    let (_, losses) = recipe_to_xmp_with_losses(&recipe);
    assert_eq!(
        losses.iter().filter(|l| l.reason == MaskLossReason::RasterNotEmbedded).count(),
        1,
        "one raster, one verdict: {losses:?}"
    );
}

/// The corner encoding is a bijection, and the WRITER is its other half.
/// Both of Lightroom's legal corner arrangements go out byte-identical to
/// the way they came in — `Left < Right` (probe #4) and `Left > Right`
/// (probe #8, which the model REQUIRES to carry `Angle > 0`, 6/6 in the
/// user's library, `BBOX-DECODE.md` §2.1). Sorting or clamping the box on
/// the way out destroys the second one.
///
/// MUTATION THIS CATCHES: `min`/`max` the corners in `engine_to_lr`, or
/// drop the `|θ| ≤ 45°` canonicalisation, and probe #8 comes back as a
/// different ellipse.
#[test]
fn the_radial_corner_encoding_round_trips_both_lightroom_arrangements() {
    let frame = FrameAspect::from_size(9504.0, 6336.0);
    for (t, l, b, r, angle) in [
        ("-0.045582", "-0.056408", "0.9708", "1.062771", "24.348422"),
        ("-0.088191", "0.520214", "1.113528", "0.494492", "29.513785"),
        // `P19`, the subject the whole angle model was measured on.
        ("-0.082402", "-0.008723", "1.109604", "1.090228", "28.229232"),
        // …and an UNROTATED one, which must not acquire an angle.
        ("0.069396", "-0.059577", "0.855822", "1.06594", "0"),
    ] {
        let doc = in_frame(
            &lr_doc(&lr_correction("R", "", &lr_radial_at(t, l, b, r, angle))),
            9504,
            6336,
        );
        let recipe = xmp_to_recipe(&doc);
        let out = recipe_to_xmp_in_frame(&recipe, frame).0;
        let shown = &out[out.find("CircularGradient").unwrap_or(0)..];
        let shown = &shown[..shown.len().min(400)];
        // The four CORNERS come back to the byte.
        for (key, want) in [("Top", t), ("Left", l), ("Bottom", b), ("Right", r)] {
            assert!(
                out.contains(&format!("crs:{key}=\"{want}\"")),
                "crs:{key} must come back as {want}: {shown}"
            );
        }
        // The ANGLE cannot, and the reason is arithmetic rather than
        // geometric: `MaskGeometry::Radial::angle` is an `f32`, whose
        // ~6 × 10⁻⁸ relative precision is 2 × 10⁻⁶ of a 34° engine angle
        // and 7.6 × 10⁻⁶ of a 73° one — a unit or two in the sixth decimal
        // Lightroom writes. Measured here: 2 × 10⁻⁶ ° on `#4`, the worst
        // 1.4 × 10⁻⁵ ° on `#8` (whose engine angle is −73.198° and whose
        // decode goes through the ±45° axis swap). Bounded at 10⁻⁴ °, four
        // orders under the +0.33° systematic the tilt measurement that
        // calibrated this carries.
        let at = out.find("crs:Angle=\"").expect("an angle is written") + 11;
        let got: f64 = out[at..][..out[at..].find('"').unwrap()].parse().expect("a number");
        let want: f64 = angle.parse().unwrap();
        assert!((got - want).abs() < 1e-4, "crs:Angle {got} vs {want}: {shown}");
    }
}

/// The decode's own guard, and the one thing it still refuses: a
/// DEGENERATE fold. The sign law — `Left > Right` forces `Angle > 0`,
/// `Top > Bottom` forces `Angle < 0`, both at once impossible, and the
/// library agrees 16/16, p = 2.5 × 10⁻⁵ — is a true statement about what
/// Lightroom WRITES. It was ALSO wired up as a readability gate, and
/// me6-2026-09 group D refuted that half: Lightroom draws the ellipse of
/// the folded MAGNITUDES ([`RadialDecode::Refused`] carries the numbers).
/// So an inverted pair imports, and only a zero fold is refused and NAMED.
///
/// MUTATION THIS CATCHES: delete the guard and a zero-area box imports as
/// a hairline (the renderer's `max(1e-4)`) with no disclosure at all;
/// put the sign half of it back and the group-D shape stops rendering.
#[test]
fn a_radial_whose_corners_do_not_decode_is_refused_and_named() {
    // A zero-AREA box: both corner pairs coincide, so the fold is (0, 0) at
    // every tilt. No ellipse, however it is turned.
    let doc = in_frame(
        &lr_doc(&lr_correction(
            "Impossible",
            "",
            &lr_radial_at("0.500000", "0.500000", "0.500000", "0.500000", "-29.513785"),
        )),
        9504,
        6336,
    );
    assert!(xmp_to_recipe(&doc).masks.is_empty(), "a box that cannot decode must not render");
    assert_eq!(unsupported_corrections(&doc), 1, "and it is counted as a drop");
    assert_eq!(
        import_losses(&doc),
        vec![MaskImportLoss {
            name: "Impossible".into(),
            reason: MaskImportReason::OutOfModel
        }],
        "…and NAMED"
    );
    // A zero-WIDTH box at zero tilt is degenerate for the same reason — at
    // θ = 0 the fold is the box. Under a tilt the SAME corners are not
    // degenerate at all: Lightroom stores the ROTATED corners, so a stored
    // width of zero folds to the honest pair (Y·sinθ, Y·cosθ). Both arms
    // are asserted here so that neither can drift into the other.
    let flat = |angle: &str| {
        in_frame(
            &lr_doc(&lr_correction(
                "Flat",
                "",
                &lr_radial_at("0.300000", "0.500000", "0.700000", "0.500000", angle),
            )),
            9504,
            6336,
        )
    };
    assert!(xmp_to_recipe(&flat("0")).masks.is_empty(), "zero-width at θ = 0 must not render");
    assert_eq!(unsupported_corrections(&flat("0")), 1, "zero-width at θ = 0 is a drop");
    assert_eq!(xmp_to_recipe(&flat("-29.513785")).masks.len(), 1, "its tilted fold is an ellipse");
    // A box the sign law says Lightroom never writes still RENDERS — the
    // refusal is about degeneracy, not about the file being unusual. Both
    // tilts of the same corners import, and each is a real ellipse rather
    // than the hairline a degenerate fold would leave.
    let shape = |angle: &str| {
        let doc = in_frame(
            &lr_doc(&lr_correction(
                "Fine",
                "",
                &lr_radial_at("-0.088191", "0.520214", "1.113528", "0.494492", angle),
            )),
            9504,
            6336,
        );
        let masks = xmp_to_recipe(&doc).masks;
        assert_eq!(masks.len(), 1, "angle {angle} did not import");
        assert_eq!(unsupported_corrections(&doc), 0, "angle {angle} was still counted a drop");
        engine_pixel_ellipse(&masks[0].mask, 9504.0, 6336.0)
    };
    for angle in ["-29.513785", "29.513785"] {
        let (major, minor, _) = shape(angle);
        assert!(major > 300.0 && minor > 300.0, "angle {angle} is a hairline: {major} x {minor}");
    }
}

/// The frame affine is the IDENTITY — the 2026-08-19 ruling set
/// `LR_MASK_FRAME_SCALE = 1.0` after R27 Batches 8+10 proved the old
/// `k = 1.032` was one frame's LENS-PROFILE WARP mistaken for a constant
/// (`batch10-report.md` §5: the `LensProfileEnable` toggle moves the
/// implied scale 0.984 → 0.998, and 11 dabs displace as a radial
/// distortion polynomial, not a scale). The recipe carries the sidecar's
/// STORED geometry verbatim.
///
/// The history this test used to pin, kept legible: `P47`
/// (`Feather="0"`, centre 2799 px off frame-centre) RENDERS its centre at
/// **(2571.0, 5060.0)** px (`PROBE4-FINAL.md` §2, 2880-ray edge fit) —
/// ~88 px from the stored (2638.4, 5002.8), because THAT frame's warp is
/// ≈1.0315. That warp is now UNMODELLED BY DECISION (batch10 §7.5: no
/// `.lcp` reader yet), so the recipe must hold the stored centre and the
/// 88 px is the disclosed residual, not something to bake in.
///
/// MUTATION THIS CATCHES: put any `k ≠ 1` back (1.032, or half-apply it
/// to axes only) and the verbatim assertions fail by 29–88 px.
#[test]
fn the_frame_affine_is_the_identity_since_the_lens_warp_ruling() {
    let (w, h) = (9504.0, 6336.0);
    let doc = in_frame(
        &lr_doc(&lr_correction(
            "R",
            "",
            &lr_radial_at("0.597862", "0.009087", "0.981315", "0.546133", "0"),
        )),
        9504,
        6336,
    );
    let MaskGeometry::Radial { top, left, bottom, right, .. } =
        xmp_to_recipe(&doc).masks[0].mask
    else {
        panic!("expected a radial");
    };
    let (cx, cy) = (
        (left as f64 + right as f64) / 2.0 * w,
        (top as f64 + bottom as f64) / 2.0 * h,
    );
    // The STORED centre, verbatim.
    let stored = ((0.009087 + 0.546133) / 2.0 * w, (0.597862 + 0.981315) / 2.0 * h);
    assert!((cx - stored.0).abs() < 0.01, "centre x {cx} px must be the stored {}", stored.0);
    assert!((cy - stored.1).abs() < 0.01, "centre y {cy} px must be the stored {}", stored.1);
    // …and PROBE4's warped-render measurement stays ~88 px away — the
    // known, disclosed, unmodelled lens warp of that frame.
    assert!(
        (2571.0f64 - cx).hypot(5060.0 - cy) > 80.0,
        "the warp residual on P47 is real and unmodelled: ({cx}, {cy})"
    );
}

/// v0.32.0 narrowed the rotation disclosure to "the document declares no
/// frame" — but there are TWO ways the tilt fails to arrive, and the frame
/// narrows only one. An angle that cannot be PARSED is a rotation nobody
/// can apply however well the frame is known: `parse_one_correction` reads
/// it as 0, so the mask arrives axis-aligned and the file's own intent is
/// gone. That has to keep saying so.
///
/// MUTATION THIS CATCHES: gate the reason on `frame.is_none()` alone and
/// this correction imports rotated-to-zero in silence.
#[test]
fn an_unreadable_angle_is_disclosed_even_when_the_frame_is_known() {
    let doc = in_frame(
        &lr_doc(&lr_correction(
            "Garbled",
            "",
            &lr_radial_at("-0.045582", "-0.056408", "0.9708", "1.062771", "twenty-four"),
        )),
        9504,
        6336,
    );
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "the shape is still readable, so the mask arrives");
    let MaskGeometry::Radial { angle, .. } = r.masks[0].mask else { panic!("radial") };
    assert_eq!(angle, 0.0, "an angle we cannot parse is not an angle we can apply");
    assert_eq!(
        import_losses(&doc),
        vec![MaskImportLoss {
            name: "Garbled".into(),
            // `0` is this payload's word for "no angle to name" — see the
            // variant's doc.
            reason: MaskImportReason::Rotation(0)
        }],
        "…and it is NAMED, frame or no frame"
    );
}

/// `crs:LocalHue` rides a 180 scale, not the ÷100 every other local key
/// uses. MEASURED, 2026-08-18: the user's controlled export put the mask
/// Hue slider at **+50** and Lightroom wrote **`crs:LocalHue="0.277778"`**
/// (`P14.xmp`, verbatim). 0.277778 × 180 = 50.00004; ÷100 would read
/// 27.8 and ÷360 would read 100.
///
/// The gate moves with the reader: at the old 100 scale a slider past
/// ±55.6 refused the WHOLE correction as out of model.
///
/// MUTATION THIS CATCHES: put `q100("LocalHue")` back and the anchor reads
/// 27.7778; leave the domain gate on the 100 scale and the ±100 row
/// refuses.
#[test]
fn local_hue_rides_the_measured_180_scale() {
    let doc = lr_doc(&lr_correction("Hue", "", &lr_radial("0", "0")))
        .replace("crs:LocalHue=\"0\"", "crs:LocalHue=\"0.277778\"");
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "the correction imports");
    assert!((r.masks[0].hue - 50.0).abs() < 1e-3, "UI +50 ⇒ {}", r.masks[0].hue);
    // The whole ±100 slider is inside the model — including its end
    // stops, which land on ±0.555556 and read back as ±100.00008 through
    // the wire's six decimals.
    for (text, want) in [("0.555556", 100.0), ("-0.555556", -100.0)] {
        let doc = lr_doc(&lr_correction("Hue", "", &lr_radial("0", "0")))
            .replace("crs:LocalHue=\"0\"", &format!("crs:LocalHue=\"{text}\""));
        let r = xmp_to_recipe(&doc);
        assert_eq!(r.masks.len(), 1, "{text} is inside Lightroom's own slider");
        assert!((r.masks[0].hue - want).abs() < 1e-2, "{text} ⇒ {}", r.masks[0].hue);
    }
    // …and a value PAST it is out of model and refused, which is the half
    // the gate exists for: 0.7 is a hue of 126, half a turn's worth beyond
    // the slider. On the old 100 scale the same file read 70 and sailed
    // through.
    let doc = lr_doc(&lr_correction("Hue", "", &lr_radial("0", "0")))
        .replace("crs:LocalHue=\"0\"", "crs:LocalHue=\"0.7\"");
    assert!(xmp_to_recipe(&doc).masks.is_empty(), "a hue past the slider is out of model");
    assert_eq!(
        import_losses(&doc),
        vec![MaskImportLoss { name: "Hue".into(), reason: MaskImportReason::OutOfModel }],
        "…and NAMED"
    );
    // …and the writer is its inverse.
    let mine = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Radial {
                top: 0.3, left: 0.35, bottom: 0.7, right: 0.65,
                feather: 0.5, roundness: 0.0, flipped: false, angle: 0.0,
                midpoint: 50.0, mask_version: 2,
            },
            hue: 50.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let out = recipe_to_xmp(&mine);
    assert!(out.contains("crs:LocalHue=\"0.2777778\""), "{out}");
    assert!((xmp_to_recipe(&out).masks[0].hue - 50.0).abs() < 1e-3);
}

// ── R27: the crop rectangle is the SAME rotated-corner encoding ─────────
//
// The seven rows below are the user's own library, transcribed verbatim
// from `p3-scratch/final_table.py:10-18` in the R27 materials ledger, outside the tree
// (the script `P3-cropangle-model.md` §8 lists as its reproduction). Each
// is a self-consistent pair: the crop block and the pixels come out of the
// same file, so a stale sidecar cannot contaminate it. NO photograph and
// NO export is in this repository — these are the metadata numbers and the
// exported dimensions they predict.
//
// `(name, W, H, L, T, R, B, CropAngle, exported W × H in SENSOR
//  orientation, px of width and height this engine's composition clamps
//  away)`.
type P3Crop = (&'static str, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64);
const P3_CROPS: [P3Crop; 7] = [
    ("P11", 9504.0, 6336.0, 0.119389, 0.003478, 0.994441, 0.991209, -0.132584, 8302.0, 6277.0, 0.000, 0.000),
    ("P30_1", 9504.0, 6336.0, 0.00219, 0.007883, 0.99781, 0.992117, -0.303486, 9429.0, 6286.0, 0.000, 0.216),
    ("P45", 9504.0, 6336.0, 0.013441, 0.0, 0.986559, 1.0, 0.724343, 9328.0, 6219.0, 0.000, 1.243),
    ("P40_hdr", 9438.0, 6265.0, 0.005237, 0.018724, 0.994763, 0.981276, -0.725680, 9262.0, 6148.0, 0.000, 1.251),
    ("P41", 9504.0, 6336.0, 0.031274, 0.0, 0.968726, 1.0, 1.728388, 9097.0, 6065.0, 0.000, 6.909),
    ("P44_1", 9504.0, 6336.0, 0.094153, 0.135618, 0.978448, 0.955195, -1.979145, 8220.0, 5480.0, 0.000, 30.260),
    ("P08", 9504.0, 6336.0, 0.09115, 0.100566, 1.0, 0.900897, -3.274380, 8334.0, 5556.0, 5.895, 0.000),
];

/// §0 OF R27 BATCH-3, the crop half. `crs:Crop{Left,Top,Right,Bottom}` are
/// the two opposite ROTATED CORNERS of the crop rectangle in the un-rotated
/// source frame — the identical encoding `BBOX-DECODE.md` found for
/// `Mask/CircularGradient`, one family — and the exported dimensions are
/// that rectangle's own side lengths, `2p × 2q`.
///
/// **7/7 pixel-exact, zero free parameters**, across two signs of
/// `CropAngle`, two `tiff:Orientation` states and two source aspect ratios
/// (`P3-cropangle-model.md` §3.2). The rivals, on the same seven rows:
/// naive AABB (what this build read until R27) 485 px, 0/7; opposite sign
/// 987 px; rotation in normalised space 165 px; AABB of the rotated rect
/// 957 px; fractions of the rotated bbox 619 px; of the inscribed rect
/// 878 px; side length ÷cos θ 477 px. The first two are asserted below, so
/// restoring either reading fails here rather than in a photograph.
///
/// The last two columns are what this engine's own composition
/// (rotate → auto-crop to the inscribed rectangle → crop) cannot express:
/// a Lightroom rectangle pushed against the edge of the rotated frame can
/// reach outside the CENTRED inscribed rectangle. Measured, per specimen,
/// and it is 0 on the first row, ≤ 1.3 px on three more, and worst at
/// 30.3 px = 0.55 % of one edge.
///
/// MUTATION THIS CATCHES: drop the `sin`/`cos` mixing in
/// `lr_to_engine_crop` (i.e. go back to `W·(R−L) × H·(B−T)`) and every row
/// but the sign-free ones misses its exported size by tens to hundreds of
/// pixels; flip the straighten's sign and the predicted sizes swap into the
/// opposite-sign column.
#[test]
fn the_seven_measured_lightroom_crops_reproduce_their_exported_dimensions() {
    for (name, w, h, l, t, r, b, angle, ow, oh, lost_w, lost_h) in P3_CROPS {
        let frame = FrameAspect::from_size(w, h);
        let lr = LrCrop { left: l, top: t, right: r, bottom: b, angle_deg: angle };
        let CropDecode::Read { crop: Some(c), straighten_deg, .. } =
            lr_to_engine_crop(lr, frame, false)
        else {
            panic!("{name}: a real Lightroom crop must decode");
        };
        assert!(
            (straighten_deg + angle).abs() < 1e-12,
            "{name}: the engine's clockwise straighten is −CropAngle"
        );
        // The frame this engine's straighten leaves behind, in pixels.
        let (wi, hi) = inscribed_norm(w / h, straighten_deg);
        let (wi, hi) = (wi * h, hi * h);
        // …and the rectangle's own side lengths inside it, with the
        // clamped edge added back: that IS the model's `2p × 2q`.
        let out_w = (c.right - c.left) as f64 * wi + lost_w;
        let out_h = (c.bottom - c.top) as f64 * hi + lost_h;
        // Half a pixel — `W_out = round(2p)` per axis — on the six
        // specimens whose crop block and pixels come out of the SAME file.
        // `P41` is the one paired to a SIDECAR, so its block may have
        // moved since the export; `P3-cropangle-model.md` §6.5 registers
        // its −0.61 px height as exactly that (and notes 9097/1.5 =
        // 6064.67, which is what an aspect lock would give).
        let tol = if name == "P41" { 0.65 } else { 0.5 };
        assert!(
            (out_w - ow).abs() < tol && (out_h - oh).abs() < tol,
            "{name}: model says {out_w:.1} × {out_h:.1}, Lightroom exported {ow} × {oh}"
        );
        // The two headline rivals, refuted on this row's own numbers.
        let naive = ((r - l) * w, (b - t) * h);
        let flipped = {
            let (sin, cos) = angle.to_radians().sin_cos();
            let (x, y) = ((r - l) / 2.0 * w, (b - t) / 2.0 * h);
            (2.0 * (x * cos - y * sin), 2.0 * (x * sin + y * cos))
        };
        if angle.abs() > 0.2 {
            assert!(
                (naive.0 - ow).abs() > 1.0 || (naive.1 - oh).abs() > 1.0,
                "{name}: the naive AABB must NOT reproduce {ow} × {oh}"
            );
            assert!(
                (flipped.0 - ow).abs() > 1.0 || (flipped.1 - oh).abs() > 1.0,
                "{name}: the opposite sign must NOT reproduce {ow} × {oh}"
            );
        }
    }
}

/// The crop codec is an ALGEBRAIC inverse: whatever Lightroom wrote comes
/// back, to the last decimal it spelled. Run on the four P3 specimens whose
/// rectangle fits this engine's inscribed frame outright, plus the two
/// arrangements §6.3 says are legal.
///
/// MUTATION THIS CATCHES: swap `p*cos − q*sin` for `p*cos + q*sin` in
/// `engine_to_lr_crop` (the natural sign slip) and every rotated row comes
/// back with different corners.
#[test]
fn the_crop_corner_encoding_round_trips_what_lightroom_wrote() {
    for (name, w, h, l, t, r, b, angle, ..) in P3_CROPS {
        let frame = FrameAspect::from_size(w, h);
        let lr = LrCrop { left: l, top: t, right: r, bottom: b, angle_deg: angle };
        let CropDecode::Read { crop: Some(c), straighten_deg, overshoot_frac } =
            lr_to_engine_crop(lr, frame, false)
        else {
            panic!("{name}: must decode");
        };
        let back = engine_to_lr_crop(Some(&c), straighten_deg, frame).expect("a crop");
        // The clamp is the only lossy edge, and it moves an edge by at
        // most its own overshoot — in units of the source height, which
        // bounds every corner coordinate (Top/Bottom are fractions of
        // that height, Left/Right of the wider width) — so that is the
        // tolerance, and it is ZERO on the rows that needed no clamp.
        let tol = overshoot_frac + 1e-6;
        for (got, want, key) in [
            (back.left, l, "Left"),
            (back.top, t, "Top"),
            (back.right, r, "Right"),
            (back.bottom, b, "Bottom"),
        ] {
            assert!(
                (got - want).abs() <= tol,
                "{name}: crs:Crop{key} {got:.6} vs {want:.6} (tol {tol:.6})"
            );
        }
        assert!((back.angle_deg - angle).abs() < 1e-12, "{name}: the angle is exact");
    }
}

/// R27 `P3-cropangle-model.md` §6.3: `Left > Right` is a legal Lightroom
/// arrangement — under the corner encoding it means `X < 0`, i.e.
/// `tan θ > p/q`, which a 2:3 crop straightened past +33.69° produces, and
/// `crs:CropAngle`'s own documented range is ±45°. The pre-R27 reader
/// required `left < right && top < bottom` and threw such a crop away in
/// silence.
///
/// The fixture is CONSTRUCTED, not copied: no file in the user's library
/// reaches the inverted region (the nine non-zero `CropAngle` sidecars all
/// sit ≥ 30.4° from their own wall), so the encoder builds the corners a
/// 35° straighten of a tall rectangle would produce and the decoder has to
/// read them back.
///
/// MUTATION THIS CATCHES: restore either half of the ordering guard and
/// the inverted arrangement decodes to `None` — a crop silently gone.
#[test]
fn an_inverted_crop_arrangement_is_read_rather_than_discarded() {
    // A tall (2:3) rectangle inside a 3:2 frame, straightened 35°.
    let frame = FrameAspect::from_size(9504.0, 6336.0);
    let engine = Crop { left: 0.30, top: 0.10, right: 0.62, bottom: 0.90 };
    let lr = engine_to_lr_crop(Some(&engine), -35.0, frame).expect("corners");
    assert!(
        lr.left > lr.right,
        "the fixture must actually reach the inverted region: {lr:?}"
    );
    let CropDecode::Read { crop: Some(back), straighten_deg, .. } =
        lr_to_engine_crop(lr, frame, false)
    else {
        panic!("an inverted arrangement must still decode: {lr:?}");
    };
    assert!((straighten_deg + 35.0).abs() < 1e-12);
    for (got, want) in [
        (back.left, engine.left),
        (back.top, engine.top),
        (back.right, engine.right),
        (back.bottom, engine.bottom),
    ] {
        assert!((got - want).abs() < 1e-5, "{got} != {want} ({back:?})");
    }
}

/// The whole document round trip on a tilted crop, through the reader and
/// the writer the app actually calls — including the `{:.6}` `CropAngle`
/// that replaced `{:.1}` (`P3-cropangle-model.md` §6.4: importing
/// `P08` and saving it back emitted `-3.3`, a 0.0256° drift = 4.3 px
/// of edge-to-edge tilt across a 9504 px frame).
///
/// MUTATION THIS CATCHES: put `{:.1}` back and the angle assertion fails
/// by the exact drift the report measured; drop the negation at either end
/// and the round trip returns `+3.274380`.
#[test]
fn a_tilted_crop_survives_a_whole_document_round_trip() {
    let doc = in_frame(
        &lr_doc(""),
        9504,
        6336,
    )
    .replace(
        "crs:Version=\"15.5.1\"",
        "crs:Version=\"15.5.1\"\n   crs:HasCrop=\"True\"\n   crs:CropLeft=\"0.09115\"\n   \
             crs:CropTop=\"0.100566\"\n   crs:CropRight=\"1\"\n   crs:CropBottom=\"0.900897\"\n   \
             crs:CropAngle=\"-3.274380\"",
    );
    let r = xmp_to_recipe(&doc);
    assert!((r.straighten_deg - 3.27438).abs() < 1e-5, "{}", r.straighten_deg);
    let c = r.crop.expect("the tilted rectangle imports");
    let out = recipe_to_xmp_in_frame(&r, FrameAspect::from_size(9504.0, 6336.0)).0;
    assert!(out.contains("crs:CropAngle=\"-3.274380\""), "{out}");
    // …and the rectangle comes back within the clamp this composition
    // costs on this specimen (5.9 px of 9186 = 0.00064).
    let back = xmp_to_recipe(&out).crop.expect("and again");
    for (got, want) in [
        (back.left, c.left),
        (back.top, c.top),
        (back.right, c.right),
        (back.bottom, c.bottom),
    ] {
        assert!((got - want).abs() < 1e-5, "{got} != {want}");
    }
}

/// A tilted crop in a FOREIGN document that declares no frame cannot be
/// placed — and is dropped and disclosed rather than read as the
/// axis-aligned rectangle it is not. Ours is read verbatim, because that
/// is precisely what this writer's frameless arm emits.
///
/// MUTATION THIS CATCHES: read the foreign one verbatim too and the note
/// disappears while a rectangle appears out of corners that mean something
/// else.
#[test]
fn a_frameless_tilted_crop_is_dropped_for_a_foreign_document_and_kept_for_ours() {
    let crop = "crs:HasCrop=\"True\" crs:CropLeft=\"0.05\" crs:CropTop=\"0\" \
                    crs:CropRight=\"0.95\" crs:CropBottom=\"1\" crs:CropAngle=\"-1.5\"";
    let foreign = format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
             xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description \
             rdf:about=\"\" xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
             {crop}/></rdf:RDF></x:xmpmeta>"
    );
    let r = xmp_to_recipe(&foreign);
    assert_eq!(r.crop, None, "a rectangle we cannot place is not invented");
    assert!((r.straighten_deg - 1.5).abs() < 1e-6, "the tilt needs no aspect and rides");
    assert!(
        crop_import_note(&foreign).is_some_and(|n| n.contains("could not be placed")),
        "{:?}",
        crop_import_note(&foreign)
    );
    // Ours: the writer's own frameless arm wrote the rectangle in the
    // straightened frame, so the reader takes it back.
    let mine = EditRecipe {
        crop: Some(Crop { left: 0.05, top: 0.0, right: 0.95, bottom: 1.0 }),
        straighten_deg: 1.5,
        ..Default::default()
    };
    let back = xmp_to_recipe(&recipe_to_xmp(&mine));
    assert_eq!(back.crop, mine.crop, "our own frameless round trip is lossless");
    assert!(crop_import_note(&recipe_to_xmp(&mine)).is_none());
}

/// The overshoot sentence counts PIXELS of the frame the straighten leaves
/// behind, on the axis the rectangle actually crossed. The decoder
/// measures the overshoot in units of the source height; the note used to
/// scale a per-axis fraction by the source's LONGER side, so a bottom-edge
/// overshoot on a 3:2 frame read 50 % too many pixels (and a width-axis
/// one was a fraction of the inscribed width scaled by the source width —
/// a different frame again).
///
/// The fixture is built through the writer: an engine crop whose bottom
/// edge sits 10 % of the straightened height BELOW that frame, folded into
/// Lightroom corners that all lie inside the source, so the reader clamps
/// it back by exactly that much.
///
/// MUTATION THIS CATCHES: `f.h` → `f.w` or `f.w.max(f.h)` in
/// `crop_import_note` (264 px), or the bare per-axis maximum back in
/// `lr_to_engine_crop` (200 px) — the sentence says 176.
#[test]
fn the_crop_overshoot_note_counts_pixels_of_the_straightened_frame() {
    let (w, h, straighten) = (3000.0, 2000.0, 5.0);
    let frame = FrameAspect::from_size(w, h);
    let engine = Crop { left: 0.2, top: 0.2, right: 0.8, bottom: 1.1 };
    let lr = engine_to_lr_crop(Some(&engine), straighten, frame).expect("corners");
    for v in [lr.left, lr.top, lr.right, lr.bottom] {
        assert!((0.0..=1.0).contains(&v), "premise: corners inside the source: {lr:?}");
    }
    let doc = in_frame(&lr_doc(""), 3000, 2000).replace(
        "crs:Version=\"15.5.1\"",
        &format!(
            "crs:Version=\"15.5.1\"\n   crs:HasCrop=\"True\"\n   crs:CropLeft=\"{}\"\n   \
                 crs:CropTop=\"{}\"\n   crs:CropRight=\"{}\"\n   crs:CropBottom=\"{}\"\n   \
                 crs:CropAngle=\"{}\"",
            lr_num(lr.left),
            lr_num(lr.top),
            lr_num(lr.right),
            lr_num(lr.bottom),
            lr_num(lr.angle_deg),
        ),
    );
    let c = xmp_to_recipe(&doc).crop.expect("the crop imports, clamped");
    assert!((c.bottom - 1.0).abs() < 1e-6, "premise: the bottom edge was clamped: {c:?}");
    // 0.1 of the straightened height, in pixels of the same scale.
    let (_, hi) = inscribed_norm(w / h, straighten);
    let want = format!("{:.0} px", 0.1 * hi * h);
    let note = crop_import_note(&doc).expect("an overshoot is disclosed");
    assert!(note.contains(&want), "want {want:?} in {note:?}");
}

/// R27 T3, the second half of `P5-cropped-mask-frame.md` §8's parser
/// lesson: `crs:RetouchAreas` carries its OWN `Mask/Ellipse` (and
/// `Mask/Paint`) components, which are healing brushes rather than local
/// adjustments — a flat `crs:What="Mask/…"` count over the packet
/// disagrees with the correction-scoped parse on **83 of 166** images in
/// the user's library. This reader has always scoped its scan to
/// `crs:MaskGroupBasedCorrections` (`mask_summary`); the point of this
/// test is that it STAYS scoped.
///
/// MUTATION THIS CATCHES: point `mask_summary` at the whole crs scope
/// instead of the correction block and the retouch ellipse arrives as a
/// phantom mask (or a phantom loss).
#[test]
fn a_retouch_area_is_not_counted_as_a_local_adjustment() {
    // RE-BASED for v1.5.0 F9. This fixture used to spell the ellipse
    // `crs:Top/Left/Bottom/Right`, which is `Mask/CircularGradient`'s
    // encoding and not this one: all 84 retouch ellipses in the reference
    // library write `crs:X/Y/SizeX/SizeY`. The old spelling did not make
    // the test wrong — it asserts that the block is not counted as a
    // correction, which holds either way — but it did mean the fixture was
    // a shape no Lightroom writes, and F9 now reads these attributes for
    // real, so it has to be the real one.
    let retouch = "  <crs:RetouchAreas>\n   <rdf:Seq>\n    <rdf:li>\n     \
             <rdf:Description crs:SpotType=\"heal\" crs:SourceState=\"sourceSetAutomatically\">\n\
             \x20    <crs:Masks>\n      <rdf:Seq>\n       <rdf:li crs:What=\"Mask/Ellipse\" \
             crs:MaskValue=\"1\" crs:X=\"0.15\" crs:Y=\"0.15\" crs:SizeX=\"0.05\" \
             crs:SizeY=\"0.05\"/>\n      </rdf:Seq>\n     </crs:Masks>\n     \
             </rdf:Description>\n    </rdf:li>\n   </rdf:Seq>\n  </crs:RetouchAreas>\n";
    let doc = lr_doc(&lr_correction("R", "", &lr_radial("0", "0")))
        .replace("  <crs:MaskGroupBasedCorrections>", &format!("{retouch}  <crs:MaskGroupBasedCorrections>"));
    assert!(doc.contains("Mask/Ellipse"), "the fixture must carry the retouch component");
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "one correction, one mask — the retouch is not one");
    assert_eq!(unsupported_corrections(&doc), 0, "nor is it a LOSS");
    assert!(import_losses(&doc).is_empty(), "{:?}", import_losses(&doc));
    // …and the other half of the same scoping fact, which is new in F9:
    // it is not a mask, and it IS a retouch area. Before this batch the
    // block read as nothing at all.
    assert_eq!(r.retouch.len(), 1, "the retouch area itself imports");
}

/// v1.5.0 F9: `crs:RetouchAreas` reads back as what Lightroom wrote — the
/// three fills and the two geometries, in the two markup forms real files
/// use for them.
///
/// Every shape here is the measured one. An ellipse is an attribute-only
/// `<rdf:li/>` (84 of 84 in the reference library) and a brush is a nested
/// `<rdf:Description>` with its own `<crs:Dabs>` (39 of 39), so this
/// fixture exercises both of the parser's paths rather than the tidier one
/// twice. The counts behind the fill names: 5 `heal`, 99 `heal_patchmatch`
/// with no `crs:fill_method`, 17 with `fill_method="firefly"`.
///
/// MUTATION THIS CATCHES: read the areas from `crs_own_scope` (which drops
/// the whole element, so nothing imports); accept an unmeasured
/// `crs:SpotType`; default `crs:Feather` to 0 instead of this engine's own;
/// treat `crs:SourceX`/`crs:OffsetY` as one-of-two-is-enough.
#[test]
fn lightrooms_spot_removal_reads_back_with_its_fill_and_its_shape() {
    use crate::retouch::{RetouchShape, SpotOrigin};
    let ellipse = |x: &str, y: &str| {
        format!(
            "<crs:Masks><rdf:Seq><rdf:li crs:What=\"Mask/Ellipse\" \
                 crs:MaskActive=\"true\" crs:MaskBlendMode=\"0\" crs:MaskInverted=\"false\" \
                 crs:MaskValue=\"1\" crs:X=\"{x}\" crs:Y=\"{y}\" crs:SizeX=\"0.05\" \
                 crs:SizeY=\"0.05\" crs:Alpha=\"0\" crs:CenterValue=\"1\" \
                 crs:PerimeterValue=\"0\"/></rdf:Seq></crs:Masks>"
        )
    };
    let paint = "<crs:Masks><rdf:Seq><rdf:Description crs:What=\"Mask/Paint\" \
             crs:MaskActive=\"true\" crs:MaskBlendMode=\"0\" crs:MaskInverted=\"false\" \
             crs:MaskSyncID=\"0000000000000000000000000000000A\" crs:MaskValue=\"1\" \
             crs:Radius=\"0.08\" crs:Flow=\"1\" crs:CenterWeight=\"0.5\">\
             <crs:Dabs><rdf:Seq><rdf:li>d 0.700000 0.300000</rdf:li></rdf:Seq></crs:Dabs>\
             </rdf:Description></rdf:Seq></crs:Masks>";
    let area = |attrs: &str, body: &str| {
        format!("<rdf:li><rdf:Description {attrs}>{body}</rdf:Description></rdf:li>")
    };
    let block = format!(
        "  <crs:RetouchAreas><rdf:Seq>{}{}{}{}{}</rdf:Seq></crs:RetouchAreas>\n",
        area(
            "crs:SpotType=\"heal\" crs:SourceState=\"sourceAutoComputed\" \
                 crs:Method=\"gaussian\" crs:HealVersion=\"2\" crs:SourceX=\"0.6\" \
                 crs:OffsetY=\"0.4\" crs:Opacity=\"1\" crs:Feather=\"0.5\" crs:Seed=\"2\"",
            &ellipse("0.25", "0.5")
        ),
        area("crs:SpotType=\"heal_patchmatch\" crs:Opacity=\"1\"", &ellipse("0.4", "0.6")),
        area(
            "crs:SpotType=\"heal_patchmatch\" crs:fill_method=\"firefly\" \
                 crs:pm_clio_model_version=\"clio-erase-2.0#test\"",
            paint
        ),
        // HALF a donor, which is not a donor. No real file does this —
        // all 5 `heal` areas state both — so this is the malformed-input
        // guard, and the 17-mutation sweep is what said it needed a case:
        // relaxing the pattern to `(Some(x), y)` changed nothing that any
        // test could see.
        area(
            "crs:SpotType=\"heal\" crs:SourceX=\"0.6\" crs:Feather=\"0.5\"",
            &ellipse("0.7", "0.7"),
        ),
        // Refused: a spelling nothing has measured. Its geometry would
        // render perfectly well; the label under it would be a claim.
        area("crs:SpotType=\"some_future_adobe_fill\"", &ellipse("0.8", "0.8")),
    );
    let doc = lr_doc(&lr_correction("R", "", &lr_radial("0", "0")))
        .replace("  <crs:MaskGroupBasedCorrections>", &format!("{block}  <crs:MaskGroupBasedCorrections>"));
    let r = xmp_to_recipe(&doc);

    assert_eq!(r.retouch.len(), 4, "four measured fills import, the unmeasured one does not");
    assert_eq!(
        r.retouch.iter().map(|a| a.origin).collect::<Vec<_>>(),
        vec![
            SpotOrigin::LightroomHeal,
            SpotOrigin::LightroomContentAware,
            SpotOrigin::LightroomGenerative,
            SpotOrigin::LightroomHeal
        ],
        "the fill axis is read from SpotType crossed with fill_method"
    );
    // Both halves of the donor or neither: the area imports, its geometry
    // stands, and the half-stated donor is simply not one.
    assert_eq!(
        r.retouch[3].donor, None,
        "a donor with one coordinate is not a donor"
    );

    // The `heal` area is the only kind that states a donor and a feather.
    let heal = &r.retouch[0];
    assert_eq!(heal.donor, Some([0.6, 0.4]), "both halves of the donor or neither");
    assert_eq!(heal.feather, 0.5);
    match heal.shape {
        RetouchShape::Ellipse { cx, cy, size_x, size_y } => {
            assert_eq!((cx, cy, size_x, size_y), (0.25, 0.5, 0.05, 0.05));
        }
        ref other => panic!("an ellipse, got {other:?}"),
    }

    // A patchmatch area states neither, so the feather is this engine's
    // own — an ABSENT attribute is not a photographer choosing zero.
    let pm = &r.retouch[1];
    assert_eq!(pm.donor, None, "Adobe states a search window, not a donor");
    assert_eq!(pm.feather, crate::retouch::HealSpot::default().feather);

    // The generative one here is a BRUSH, which is the common pairing:
    // 14 of the library's 17 firefly areas are brushes, not ellipses.
    match r.retouch[2].shape {
        RetouchShape::Brush(ref strokes) => {
            assert_eq!(strokes.len(), 1);
            assert_eq!(strokes[0].radius, 0.08);
            assert_eq!(strokes[0].dabs, "d 0.700000 0.300000");
        }
        ref other => panic!("a brush, got {other:?}"),
    }

    // And none of it reached the local adjustments.
    assert_eq!(r.masks.len(), 1, "still one correction, one mask");
}

/// v1.5.0 F8: HDR edit mode and the seven SDR controls survive a round
/// trip, in Lightroom's own spellings.
///
/// Three spellings are pinned here because getting any of them wrong
/// produces a file Lightroom reads silently and WRONGLY:
///
/// * `crs:HDREditMode` is Lightroom's `"0"`/`"1"` and not its
///   `"True"`/`"False"` — both boolean spellings live in one sidecar, and
///   `"True"` here reads as OFF;
/// * `crs:HDRMaxValue` is a two-place decimal (`"+1.00"`), the
///   `SharpenRadius` shape, not an integer;
/// * the seven `crs:SDR*` are signed integers like every Basic slider.
///
/// Every read here goes through [`bare_document`], and that is the whole
/// difference between a test and a tautology. `recipe_to_xmp` embeds the
/// recipe as an `ash` PAYLOAD and `xmp_to_recipe` prefers it, so a round
/// trip through the public pair proves the JSON survived and says nothing
/// about the `crs:` attributes — relaxing the reader to `!= Some("0")`,
/// which turns HDR mode on for every photograph in the archive, was GREEN
/// against the payload form (falsification case F8-M10).
///
/// MUTATION: write `"True"` for the mode; drop the `+.2` from the
/// headroom; read the mode with `!= Some("0")` (an absent key turns HDR
/// mode ON for every photograph in the archive).
#[test]
fn hdr_edit_mode_and_its_sdr_rendition_round_trip_in_lightrooms_spellings() {
    let r = EditRecipe {
        hdr_edit: true,
        hdr_max_ev: 1.5,
        sdr_blend: -30.0,
        sdr_brightness: 40.0,
        sdr_contrast: 15.0,
        sdr_highlights: -50.0,
        sdr_shadows: 25.0,
        sdr_whites: -20.0,
        sdr_clarity: 10.0,
        ..Default::default()
    };
    let doc = recipe_to_xmp(&r);
    for want in [
        "crs:HDREditMode=\"1\"",
        "crs:HDRMaxValue=\"+1.50\"",
        "crs:SDRBlend=\"-30\"",
        "crs:SDRBrightness=\"+40\"",
        "crs:SDRContrast=\"+15\"",
        "crs:SDRHighlights=\"-50\"",
        "crs:SDRShadows=\"+25\"",
        "crs:SDRWhites=\"-20\"",
        "crs:SDRClarity=\"+10\"",
    ] {
        assert!(doc.contains(want), "missing {want} in\n{doc}");
    }
    // Read back from the PAYLOAD-FREE projection, so what comes home is
    // the nine attributes above and not the embedded JSON.
    let back = xmp_to_recipe(&bare_document(&r, None));
    assert!(back.hdr_edit, "the mode comes home");
    assert_eq!(back.hdr_max_ev, 1.5, "…and the headroom, to its two places");
    assert_eq!(back.sdr_controls(), r.sdr_controls(), "…and all seven controls");

    // The mode is read STRICTLY: absent is off, "0" is off, and so is
    // Lightroom's other boolean spelling, which this key never uses.
    let off = EditRecipe::default();
    let bare = bare_document(&off, None);
    assert!(!bare.contains("HDREditMode") && !bare.contains("SDR"), "{bare}");
    assert!(!xmp_to_recipe(&bare).hdr_edit, "an absent key is not HDR mode");
    let inject = |spelling: &str| {
        bare.replace("crs:Version=", &format!("crs:HDREditMode=\"{spelling}\" crs:Version="))
    };
    // THE POSITIVE CONTROL, and the reason it is here: without it the four
    // negatives below are satisfied by an injection that never reached the
    // reader at all, and relaxing the read to `!= Some("0")` — which turns
    // HDR mode ON for every photograph in the archive — stayed green
    // (falsification case F8-M10). Prove the door opens before testing
    // that it is shut.
    assert!(
        xmp_to_recipe(&inject("1")).hdr_edit,
        "premise: an injected crs:HDREditMode must reach the reader"
    );
    for spelling in ["0", "True", "true", ""] {
        assert!(
            !xmp_to_recipe(&inject(spelling)).hdr_edit,
            "crs:HDREditMode=\"{spelling}\" is not HDR mode"
        );
    }
}

/// The same nine keys read from LIGHTROOM's bytes:
/// `src/fixtures/hdr-on-lightroom-9.4.xmp`, which Lightroom 9.4 (Camera
/// Raw 18.4) wrote on 2026-09-19 with the mode on.
///
/// The round trip above proves this writer and this reader agree with
/// each other, which is a weaker claim than agreeing with Lightroom, and
/// no library file can make the stronger one: the reference library's
/// 175 sidecars carry `crs:HDREditMode="0"` on 114 photographs and `"1"`
/// on NONE (census 2026-09-17, [`crate::render::hdr`]). This one file is
/// also the measurement's own input --- the headroom shoulder in
/// `render::hdr` was fitted against its `+2.30` (rms 0.0412 of
/// Lightroom's own transfer), so a tree without it cannot re-derive that
/// number. It is 8,165 bytes and names no file, place or person; what it
/// carries beyond the develop is the camera and the lens.
///
/// MUTATION THIS CATCHES: read the headroom with a parser that stops at
/// the sign (`+2.30` -> 0.0, a photograph with no headroom at all), or
/// let an SDR control fall back to its default instead of reading the
/// zero Lightroom wrote.
#[test]
fn lightrooms_own_hdr_sidecar_reads_as_the_mode_and_headroom_it_states() {
    let doc = include_str!("../fixtures/hdr-on-lightroom-9.4.xmp");
    // The premise, and what makes this test different from the one
    // above: Lightroom wrote this file, so there is no payload of ours
    // to read the develop out of. Everything below is `crs:` attributes.
    assert!(
        !doc.contains("xmlns:asr"),
        "a Lightroom-written sidecar carries no payload of ours"
    );
    assert!(
        doc.contains("crs:HDREditMode=\"1\""),
        "premise: the mode is on in the bytes themselves"
    );

    let r = xmp_to_recipe(doc);
    assert!(r.hdr_edit, "the mode Lightroom wrote comes home");
    assert_eq!(r.hdr_max_ev, 2.30, "...and the headroom, sign and two places");
    assert_eq!(
        r.sdr_controls(),
        [0.0; 7],
        "...and all seven SDR controls, at the zero Lightroom wrote"
    );
}

/// R27 T4, `P2-feather-k-closures.md` §4.3. Lightroom's rule for the
/// detail/NR companions is **amount-gated**: 4/4 exports with
/// `LuminanceSmoothing > 0` carry the Detail companion and 0 of 207 with
/// it at 0 do; `SharpenEdgeMasking="0"` rides on 159 files whose
/// `Sharpness` is set. This writer gated each companion on its OWN value,
/// so `P33.JPG`'s shape — `LuminanceSmoothing="50"`,
/// `…Detail="50"`, `…Contrast="0"` — came out with the Contrast key
/// missing, which no Lightroom file has.
///
/// Behaviourally neutral by construction: only the companions whose ACR
/// default is ZERO join the rule (see `amount_carries`), so an emitted
/// `"0"` says exactly what its absence said.
///
/// MUTATION THIS CATCHES: revert `amount_carries` to `false` and the two
/// zero-valued companions vanish from a document whose amounts are set;
/// widen it to every companion and `SharpenRadius="+0.0"` starts asserting
/// a radius Lightroom would render at.
#[test]
fn the_detail_companions_ride_on_their_amount_the_way_lightroom_writes_them() {
    // `P33.JPG`, verbatim (P2 §4.1).
    let r = EditRecipe {
        noise_reduction: 50.0,
        nr_detail: 50.0,
        nr_contrast: 0.0,
        color_nr: 100.0,
        color_nr_detail: 50.0,
        color_nr_smooth: 50.0,
        sharpening: 0.0,
        ..Default::default()
    };
    let x = recipe_to_xmp(&r);
    for want in [
        "crs:LuminanceSmoothing=\"50\"",
        "crs:LuminanceNoiseReductionDetail=\"50\"",
        "crs:LuminanceNoiseReductionContrast=\"0\"",
        "crs:ColorNoiseReduction=\"100\"",
        "crs:ColorNoiseReductionDetail=\"50\"",
        "crs:ColorNoiseReductionSmoothness=\"50\"",
    ] {
        assert!(x.contains(want), "the LR-shaped NR block is missing {want}: {x}");
    }
    assert!(!x.contains("SharpenEdgeMasking"), "no sharpening amount, no companion: {x}");
    assert!(!x.contains("SharpenRadius"), "and never an invented radius: {x}");
    // Sharpening set, masking at rest: Lightroom writes the zero.
    let sharp = EditRecipe { sharpening: 45.0, ..Default::default() };
    let x = recipe_to_xmp(&sharp);
    assert!(x.contains("crs:SharpenEdgeMasking=\"0\""), "{x}");
    assert!(!x.contains("SharpenDetail"), "a 25-default companion stays absent: {x}");
    assert!(!x.contains("SharpenRadius"), "so does the 1.0-default radius: {x}");
    // A recipe with NO noise reduction keeps the whole block absent — the
    // half of the rule the old per-key gate got right.
    let none = recipe_to_xmp(&EditRecipe::default());
    assert!(!none.contains("LuminanceNoiseReductionContrast"), "{none}");
    assert!(!none.contains("SharpenEdgeMasking"), "{none}");
}

/// R27 A7. A PORTRAIT capture's `crs:` coordinates are fractions of the
/// UN-ROTATED SENSOR ARRAY, and the export is already upright
/// (`P1-portrait-mask-frame.md` §1, HIGH; 7/7 files pick their true frame
/// by `dSS`, four landscape controls recover the known answer).
///
/// The fixture is `P46-已增强-NR.JPG`'s Mask 8 (P1 §4.2), verbatim:
/// a single `Mask/CircularGradient` at `Angle="0"` declaring +1.6 EV, whose
/// `|b|` far exceeds the frame so it renders as a BAND — and the two
/// readings put that band on perpendicular axes, 12 628 px apart. P1
/// measures its fitted gain at **+1.647 ± 0.270** under the sensor frame
/// against a declared +1.60, and **−1.982** — the wrong SIGN — under the
/// display one.
///
/// The two numbers asserted here are P1 §4.2's own: the band sits about
/// display `y = 4748` with a half-extent of `4315` px along that axis.
/// Under the rejected reading it would be a VERTICAL band about
/// `x = 3171` with half-width `4453`.
///
/// MUTATION THIS CATCHES: drop the `orient_recipe_coords` call at the end
/// of `xmp_to_recipe` (or read `tiff:Orientation` as `Normal`) and the band
/// comes back vertical — the v0.33 defect P1 was written to close.
#[test]
fn a_portrait_captures_mask_is_decoded_in_the_sensor_frame() {
    // sensor 9504 × 6336, tiff:Orientation="8" → display 6336 × 9504.
    let doc = in_frame(
        &lr_doc(&lr_correction(
            "Mask 8",
            "",
            &lr_radial_at("-3.21675", "0.060424", "2.073933", "0.940417", "0"),
        )),
        9504,
        6336,
    )
    .replace("tiff:ImageWidth", "tiff:Orientation=\"8\"\n   tiff:ImageWidth");
    let r = xmp_to_recipe(&doc);
    let MaskGeometry::Radial { top, left, bottom, right, .. } = r.masks[0].mask else {
        panic!("a radial");
    };
    // The engine's frame is the DISPLAY one: 6336 × 9504.
    let (cy, ry) = (
        (top as f64 + bottom as f64) / 2.0 * 9504.0,
        (bottom as f64 - top as f64) / 2.0 * 9504.0,
    );
    assert!((cy - 4748.0).abs() < 1.5, "the band's display centre is y = {cy:.0}, not 4748");
    // P1 §4.2's own pixel number was 4315 — measured on the EXPORT, i.e.
    // the k-image (4315 = 1.032 × 4182). Since the 2026-08-19 ruling set
    // `LR_MASK_FRAME_SCALE = 1.0` the recipe carries the STORED extent,
    // 0.4399965 × 9504 = 4181.7; the residual vs the export is that
    // frame's lens-profile warp, unmodelled by decision (batch10 §7.5).
    assert!((ry - 4182.0).abs() < 1.5, "its display half-extent is {ry:.0}, not 4182");
    // …and the OTHER axis is the one that overflows the frame, which is
    // what makes it a band rather than an ellipse.
    let rx = (right as f64 - left as f64) / 2.0 * 6336.0;
    assert!(rx > 6336.0, "the perpendicular half-extent {rx:.0} must exceed the frame");
    // The writer un-turns it: the file's own four numbers come back.
    let out = recipe_to_xmp_in_frame(
        &r,
        FrameAspect::from_size_turned(9504.0, 6336.0, rawler::Orientation::Rotate270),
    )
    .0;
    for (key, want) in [
        ("Top", "-3.21675"),
        ("Left", "0.060424"),
        ("Bottom", "2.073933"),
        ("Right", "0.940417"),
    ] {
        assert!(
            out.contains(&format!("crs:{key}=\"{want}\"")),
            "crs:{key} did not survive the portrait round trip: {out}"
        );
    }
}

/// R29 C1, the sidecar boundary — the brush twin of the radial test above.
///
/// A portrait capture's `crs:Dabs` are fractions of the UN-ROTATED SENSOR
/// array, exactly like its `crs:Top/Left`, so the reader has to turn them
/// into the display frame and the writer has to turn them back. Until this
/// batch neither happened: the stream was carried verbatim, which made the
/// round trip byte-exact and the RENDER wrong by a quarter turn — invisible
/// while the brush drew nothing (R27 Batch-4) and visible the moment it did
/// (R29 Batch-6b).
///
/// The rewrite is not free, and this test is where the cost is legible: the
/// stream that comes back out was COMPUTED, not copied. It survives here
/// because Lightroom writes six decimals and this writer re-emits six
/// (`render::LR_DAB_DECIMALS`), so on the pure rotations the decimal grid
/// is closed under the turn — `1 − 0.800000` is `0.200000` both ways, and
/// `0.582157 × 1.5 ÷ 1.5` lands back on `0.582157`. It is arithmetic that
/// happens to be exact, not a carry, and a frame whose aspect is not 3:2
/// would come home within a millionth instead of on it.
///
/// MUTATIONS THIS CATCHES: drop the frame argument at either boundary (the
/// import leaves the dabs sensor-frame, so the export turns them once and
/// they leave the file rotated); hand `in_source_frame` the SOURCE aspect
/// instead of `displayed()` (the radius comes back 1.31, i.e. 1.5² × the
/// error).
#[test]
fn a_portrait_captures_brush_turns_on_the_way_in_and_comes_home_on_the_way_out() {
    // sensor 9504 × 6336, tiff:Orientation="8" = Rotate270 → display
    // 6336 × 9504. `lr_paint` pins crs:Radius="0.582157" (the F2 specimen).
    let paint = lr_paint(
        "FA7459A9F5626F4881D7B730C3093F95",
        "1",
        "0",
        "false",
        &["r 0.200000", "d 0.100000 0.800000"],
    );
    let group = format!(
        "<rdf:li>\n\
             <rdf:Description crs:What=\"Mask/Aggregate\" crs:MaskActive=\"true\"\n\
             crs:MaskName=\"Brush 1\" crs:MaskBlendMode=\"0\" crs:MaskInverted=\"false\"\n\
             crs:MaskSyncID=\"0000000000000000000000000000000D\" crs:MaskValue=\"1\">\n\
             <crs:Masks>\n<rdf:Seq>\n{paint}</rdf:Seq>\n</crs:Masks>\n\
             </rdf:Description>\n</rdf:li>\n"
    );
    let doc = in_frame(&lr_doc(&lr_correction("Mask 7", "", &group)), 9504, 6336)
        .replace("tiff:ImageWidth", "tiff:Orientation=\"8\"\n   tiff:ImageWidth");
    let r = xmp_to_recipe(&doc);
    let stroke = |r: &EditRecipe| {
        let g = &r.masks[0].mask;
        let MaskGeometry::Brush { strokes, .. } = g else { panic!("a brush, got {g:?}") };
        (strokes[0].dabs.clone(), strokes[0].radius)
    };
    // IN: Rotate270 maps (u,v) -> (v, 1−u), and the source aspect 1.5
    // rescales every width-unit radius.
    let (dabs, radius) = stroke(&r);
    assert_eq!(dabs, "r 0.300000\nd 0.800000 0.900000", "the stream must reach the display frame");
    assert!((radius - 0.873236).abs() < 1e-6, "crs:Radius became {radius}, not 0.873236");

    // OUT: the inverse, against the DISPLAYED aspect — the file's own
    // digits come back.
    let out = recipe_to_xmp_in_frame(
        &r,
        FrameAspect::from_size_turned(9504.0, 6336.0, rawler::Orientation::Rotate270),
    )
    .0;
    for want in [
        "<rdf:li>r 0.200000</rdf:li>",
        "<rdf:li>d 0.100000 0.800000</rdf:li>",
        "crs:Radius=\"0.582157\"",
    ] {
        assert!(out.contains(want), "{want} did not survive the portrait round trip: {out}");
    }
}

/// R27 A8, and it closes `C-rotation-skeleton.md`'s round-trip hole: a
/// document THIS writer produces now declares the frame its coordinates
/// are measured against, so re-importing one recovers the rotated radial
/// it wrote. Before R27 a fresh sidecar declared nothing, and the reader —
/// which needs `W/H` to fold a pixel-frame tilt into the engine's
/// normalised one — could not read our own file back.
///
/// MUTATION THIS CATCHES: return an empty string from `frame_declaration`
/// and the re-import loses the tilt (and says so through
/// `describe_import_losses`), which is exactly the hole.
#[test]
fn a_fresh_document_declares_its_frame_and_can_be_read_back() {
    let r = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Radial {
                top: 0.30, left: 0.20, bottom: 0.62, right: 0.75,
                feather: 0.5, roundness: 0.0, flipped: false, angle: 21.5,
                midpoint: 50.0, mask_version: 2,
            },
            exposure_ev: 0.4,
            ..Default::default()
        }],
        crop: Some(Crop { left: 0.08, top: 0.05, right: 0.9, bottom: 0.94 }),
        straighten_deg: 1.25,
        ..Default::default()
    };
    for turn in [rawler::Orientation::Normal, rawler::Orientation::Rotate270] {
        let frame = FrameAspect::from_size_turned(9504.0, 6336.0, turn);
        let doc = recipe_to_xmp_in_frame(&r, frame).0;
        assert!(doc.contains("xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\""), "{doc}");
        assert!(doc.contains("tiff:ImageWidth=\"9504\""), "{doc}");
        assert!(doc.contains("tiff:ImageLength=\"6336\""), "{doc}");
        assert!(
            doc.contains(&format!("tiff:Orientation=\"{}\"", turn.to_u16())),
            "the DISPLAYED orientation, quarter turns included: {doc}"
        );
        let back = xmp_to_recipe(&doc);
        // Compared as the ELLIPSE it draws, in the frame this engine
        // displays — `(box, angle)` is a redundant carrier (a quarter turn
        // of the box with the angle shifted 90° is the SAME ellipse), and
        // Lightroom's own ±45° canonicalisation legitimately picks the
        // other representative on the way through.
        let (dw, dh) = if crate::decode::orientation_transposes(turn) {
            (6336.0, 9504.0)
        } else {
            (9504.0, 6336.0)
        };
        let (a0, b0, t0) = engine_pixel_ellipse(&r.masks[0].mask, dw, dh);
        let (a1, b1, t1) = engine_pixel_ellipse(&back.masks[0].mask, dw, dh);
        assert!(
            (a0 - a1).abs() < 1.0 && (b0 - b1).abs() < 1.0,
            "{turn:?}: semi-axes {a0:.1}/{b0:.1} came back {a1:.1}/{b1:.1}"
        );
        assert!(
            ((t0 - t1).rem_euclid(180.0)).min((t1 - t0).rem_euclid(180.0)) < 1e-2,
            "{turn:?}: tilt {t0} came back as {t1}"
        );
        assert!((back.straighten_deg - 1.25).abs() < 1e-4, "{turn:?}: tilt");
        let c = back.crop.expect("the crop survives");
        assert!(
            (c.left - 0.08).abs() < 2e-3 && (c.bottom - 0.94).abs() < 2e-3,
            "{turn:?}: crop {c:?}"
        );
    }
}

/// **The import half of the sidecar orientation law** (W14), arm by arm.
/// The law itself, its measurement and its sign convention live on
/// [`crate::render::quarter_turns_between`]; this pins what THIS module
/// does with the answer.
///
/// MUTATION THIS CATCHES: hand `honour_declared_orientation` the declared
/// state as its own EXIF (`quarter_turns_between(want, want)`) and every
/// row that needs a turn comes back 0 — the defect the me6-2026-09 pack
/// measured, where 46 landscape exports rendered portrait.
#[test]
fn a_sidecars_declared_orientation_becomes_the_photographers_quarter_turns() {
    use rawler::Orientation as O;
    let turn = |declared, exif| honour_declared_orientation(declared, exif);
    // No declaration: the document says nothing about which way the photo
    // is up, so neither does the recipe. Today's behaviour, byte for byte.
    assert_eq!(
        turn(None, Some(O::Rotate270)),
        HonouredTurn { turn: O::Normal, quarter_turns: 0, refused: None }
    );
    // No photograph to compose against (a baked source, an unreadable
    // RAW): the declaration is the only frame information there is and is
    // taken at face value, which is what every build before W14 did.
    assert_eq!(
        turn(Some(O::Rotate270), None),
        HonouredTurn { turn: O::Rotate270, quarter_turns: 0, refused: None }
    );
    // The me6-2026-09 pack's own row: IFD0 Orientation = 8 under 46
    // sidecars that declare 1. One clockwise quarter turn delivers the
    // landscape frame Lightroom exported.
    assert_eq!(
        turn(Some(O::Normal), Some(O::Rotate270)),
        HonouredTurn { turn: O::Normal, quarter_turns: 1, refused: None }
    );
    // Agreement costs nothing — which is why the 169 sidecar/RAW pairs of
    // the reference library, all of which agree, import unchanged.
    for o in [O::Normal, O::Rotate90, O::Rotate180, O::Rotate270, O::Transpose] {
        assert_eq!(turn(Some(o), Some(o)).quarter_turns, 0, "{o:?} agrees with itself");
    }
    // A MIRROR over an un-mirrored capture has no quarter turn at all, so
    // the tag is refused WHOLE: the photograph's own EXIF decides the
    // frame, exactly as it does for a sidecar that declares nothing, and
    // the refusal is named rather than mapped to the nearest rotation.
    assert_eq!(
        turn(Some(O::HorizontalFlip), Some(O::Normal)),
        HonouredTurn { turn: O::Normal, quarter_turns: 0, refused: Some(O::HorizontalFlip) }
    );
    // …and a mirror over a MIRRORED capture is a plain rotation, so it is
    // honoured. "Mirrored values are refused" would have been the wrong
    // rule: what cannot be delivered is a change of HANDEDNESS.
    assert_eq!(
        turn(Some(O::VerticalFlip), Some(O::HorizontalFlip)),
        HonouredTurn { turn: O::VerticalFlip, quarter_turns: 2, refused: None }
    );
}

/// **The export half**: a merge corrects the base's `tiff:Orientation` when
/// — and only when — the photographer's turn has moved away from it.
///
/// Both halves matter and they pull against each other. A save that never
/// touched the tag could not carry a rotation made here to Lightroom; a
/// save that always rewrote it would move the base's own bytes on every
/// round trip. The composed state (`pipeline::photo_frame_aspect`) decides,
/// so a recipe imported from this very sidecar composes back to the value
/// already in it and nothing is written.
///
/// MUTATION THIS CATCHES: return `FrameDecl::Keep` unconditionally from
/// `merge_frame`'s disagreement arm and the turned save leaves
/// An orientation-only document — the shape 154 of this library's 175
/// sidecars have — is read in the PHOTOGRAPH's frame, which is the frame
/// its geometry was folded through on the way out. With the RAW beside
/// it, a rotated radial comes back rotated and a tilted crop placed; the
/// document-only readers still abstain (nothing to compose against), and
/// the photo-aware disclosure doors agree with the recipe reader instead
/// of reporting a rotation loss that did not happen.
///
/// MUTATION THIS CATCHES: drop the `frame_fallback` line from the reader
/// (the radial decodes `Unrotated`), or the `photo_frame_fallback` from
/// either door (the false loss note returns).
#[test]
fn an_orientation_only_sidecar_is_read_in_the_photographs_frame() {
    let Some(root) = crate::fit::calibration_corpus() else { return };
    let raw = root.join("p36.arw");
    if !raw.is_file() {
        crate::test_skipped("orientation-only sidecar", "p36.arw not in the corpus");
        return;
    }
    let ((w, h), exif) = crate::pipeline::source_frame_memo(&raw).expect("p36's header reads");
    let frame = FrameAspect::from_size_turned(w as f64, h as f64, exif).expect("a rectangle");
    let r = EditRecipe {
        crop: Some(crate::recipe::Crop { left: 0.1, top: 0.1, right: 0.9, bottom: 0.9 }),
        straighten_deg: 3.0,
        masks: vec![LocalAdjustment {
            // Clearly elongated (0.6 w by 0.2 h): a near-circular ellipse
            // reads back with either axis as the major one, and its
            // tilt then a quarter turn off, which says nothing about
            // the frame it was read in.
            mask: MaskGeometry::Radial {
                top: 0.3,
                left: 0.2,
                bottom: 0.5,
                right: 0.8,
                feather: 0.5,
                roundness: 0.0,
                flipped: false,
                angle: 37.0,
                midpoint: 50.0,
                mask_version: 2,
            },
            exposure_ev: 0.5,
            ..Default::default()
        }],
        ..Default::default()
    };
    // The projection through the photograph's frame with no payload to
    // restore from, then the frame block reduced to the orientation alone:
    // exactly what a merge into a Lightroom base that declares only
    // `tiff:Orientation` writes (`merge_frame`'s Keep arm).
    let full = bare_document(&r, Some(frame));
    let doc = full
        .lines()
        .filter(|l| !l.contains("tiff:ImageWidth=") && !l.contains("tiff:ImageLength="))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(doc.contains("tiff:Orientation=") && !doc.contains("tiff:ImageWidth"), "premise: {doc}");
    assert_eq!(FrameAspect::from_xmp(&doc), None, "premise: the document declares no rectangle");
    // The recipe reader, with the photograph: the rotation and the crop come home.
    let with_photo = xmp_to_recipe_for_photo(&doc, &raw);
    let MaskGeometry::Radial { angle, .. } = with_photo.masks[0].mask else { panic!("a radial") };
    assert!((angle - 37.0).abs() < 0.5, "the rotation is read in the photo's frame: {angle}");
    let crop = with_photo.crop.expect("the tilted crop is placed");
    assert!((crop.left - 0.1).abs() < 0.02 && (crop.right - 0.9).abs() < 0.02, "{crop:?}");
    // …and without it the reader abstains exactly as before.
    let MaskGeometry::Radial { angle: blind, .. } = xmp_to_recipe(&doc).masks[0].mask else { panic!() };
    assert_eq!(blind, 0.0, "no photograph, no frame, no rotation");
    // The disclosure doors agree with the reader they describe.
    assert!(
        import_losses(&doc).iter().any(|l| matches!(l.reason, MaskImportReason::Rotation(_))),
        "premise: the document-only door reports the rotation as lost"
    );
    assert!(
        !import_losses_for_photo(&doc, &raw).iter().any(|l| matches!(l.reason, MaskImportReason::Rotation(_))),
        "with the photograph the rotation imported, so it is not a loss"
    );
    // (The crop half of the same door is stated on Lightroom's own
    // documents by `the_census_has_no_false_rotation_loss_beside_a_readable_raw`:
    // an AutoShade-authored document places its rectangle through the
    // verbatim arm with no frame at all.)
    assert!(
        crop_import_note_for_photo(&doc, &raw).is_none_or(|n| !n.contains("could not be placed")),
        "with the photograph the rectangle is placed"
    );
}

/// The library census, read through both doors: on every orientation-only
/// sidecar beside a readable RAW the photo-aware door reports no rotation
/// loss the document-only door invented. Runs only where the census is
/// (`AUTOSHADE_CENSUS_ROOT`), and prints its counts for the ledger.
#[test]
fn the_census_has_no_false_rotation_loss_beside_a_readable_raw() {
    let Some(root) = std::env::var_os("AUTOSHADE_CENSUS_ROOT") else {
        crate::test_skipped("census rotation losses", "AUTOSHADE_CENSUS_ROOT unset");
        return;
    };
    let mut sidecars = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xmp")) {
                sidecars.push(p);
            }
        }
    }
    let (mut orientation_only, mut with_raw, mut doc_rotation, mut photo_rotation, mut doc_crop, mut photo_crop) =
        (0, 0, 0, 0, 0, 0);
    for xmp in &sidecars {
        let Ok(text) = std::fs::read_to_string(xmp) else { continue };
        if declared_orientation(&text).is_none() || FrameAspect::from_xmp(&text).is_some() {
            continue;
        }
        orientation_only += 1;
        let Some(raw) = ["arw", "ARW", "dng", "DNG"]
            .iter()
            .map(|ext| xmp.with_extension(ext))
            .find(|p| p.is_file())
        else {
            continue;
        };
        if crate::pipeline::source_frame_memo(&raw).is_none() {
            continue;
        }
        with_raw += 1;
        let rot = |l: &[MaskImportLoss]| l.iter().any(|l| matches!(l.reason, MaskImportReason::Rotation(_)));
        doc_rotation += usize::from(rot(&import_losses(&text)));
        photo_rotation += usize::from(rot(&import_losses_for_photo(&text, &raw)));
        let unplaced = |n: Option<String>| n.is_some_and(|n| n.contains("could not be placed"));
        doc_crop += usize::from(unplaced(crop_import_note(&text)));
        photo_crop += usize::from(unplaced(crop_import_note_for_photo(&text, &raw)));
    }
    eprintln!(
        "CENSUS orientation-only {orientation_only} of {} sidecars, {with_raw} beside a readable RAW; \
             rotation losses document-only {doc_rotation} / photo-aware {photo_rotation}; \
             unplaced crops document-only {doc_crop} / photo-aware {photo_crop}",
        sidecars.len()
    );
    assert_eq!(photo_rotation, 0, "a rotation the photo-aware reader imports is not a loss");
    assert_eq!(photo_crop, 0, "a rectangle the photo-aware reader places is not unplaceable");
}

/// `tiff:Orientation="8"` on a document whose geometry is now landscape.
#[test]
fn a_merge_corrects_the_declared_orientation_only_when_the_turn_moved() {
    use rawler::Orientation as O;
    // A Lightroom-shaped base: the sensor rectangle plus a portrait turn.
    let base = in_frame(&lr_doc(""), 9504, 6336)
        .replace("tiff:ImageWidth", "tiff:Orientation=\"8\"\n   tiff:ImageWidth");
    let r = EditRecipe { exposure_ev: 0.25, ..Default::default() };
    let merged = |turn| {
        merge_recipe_into_xmp_in_frame(
            &base,
            &r,
            FrameAspect::from_size_turned(9504.0, 6336.0, turn),
        )
        .expect("the base is mergeable")
        .doc
    };
    // Unmoved: the base's own frame block survives verbatim, in its own
    // order, with its own whitespace.
    let same = merged(O::Rotate270);
    assert_eq!(same.matches("tiff:Orientation").count(), 1, "{same}");
    assert!(
        same.contains("tiff:Orientation=\"8\"\n   tiff:ImageWidth=\"9504\""),
        "the base's frame block did not survive byte for byte: {same}"
    );
    // Turned here: the declaration follows the composed state, exactly
    // once, and the rectangle it was measured against does not move.
    let turned = merged(O::Normal);
    assert_eq!(
        turned.matches("tiff:Orientation").count(),
        1,
        "one declaration, never two: {turned}"
    );
    assert!(turned.contains("tiff:Orientation=\"1\""), "{turned}");
    assert!(turned.contains("tiff:ImageWidth=\"9504\""), "the rectangle stays: {turned}");
    // …and a reader gets each one back out.
    assert_eq!(declared_orientation(&same), Some(O::Rotate270));
    assert_eq!(declared_orientation(&turned), Some(O::Normal));
    // Without a photograph there is nothing to compose, so the base's own
    // declaration stands — a `recipe_to_xmp`-style save of a document we
    // cannot place must not invent a frame for it.
    let blind = merge_recipe_into_xmp(&base, &r).expect("mergeable").doc;
    assert!(blind.contains("tiff:Orientation=\"8\""), "{blind}");
}

/// **A corrected orientation is written only where this engine's own
/// reader will find it, and arrives bound.** [`FrameScope::resolve`] takes
/// the declaration from the one Description that declares both
/// dimensions, so a correction spliced onto a DIFFERENT Description (the
/// exiftool shape: one Description per namespace) would be invisible to
/// the reader — the round trip would lose the turn while the coordinates
/// went out in a frame nobody recovers. And an attribute whose prefix no
/// enclosing element binds makes the WHOLE sidecar unreadable to an XML
/// parser, Lightroom's included. Three shapes, one rule each:
///
/// * the frame lives in another Description ⇒ the base's declaration is
///   kept, the geometry goes out in the frame the file declares, and the
///   merge NAMES what it could not write;
/// * the crs Description declares the frame and an ancestor binds the
///   prefix ⇒ the correction lands on the crs tag WITH its own binding;
/// * the crs Description binds the prefix itself ⇒ corrected, bound once.
///
/// MUTATION THIS CATCHES: drop the scope check from `merge_frame` and
/// the first shape writes a `tiff:Orientation` the reader never sees;
/// drop the binding from `orientation_declaration` and the second shape
/// carries `tiff:` with no binding in its own start tag.
#[test]
fn a_corrected_orientation_lands_where_the_reader_looks_and_arrives_bound() {
    use rawler::Orientation as O;
    let r = EditRecipe { exposure_ev: 0.25, ..Default::default() };
    let turned = |base: &str| {
        merge_recipe_into_xmp_in_frame(
            base,
            &r,
            FrameAspect::from_size_turned(9504.0, 6336.0, O::Rotate90),
        )
        .expect("mergeable")
    };
    let crs_tag = |doc: &str| {
        let start = find_crs_description(doc).expect("the settings Description survives");
        let (gt, _) = scan_tag_end(doc, start).expect("its start tag closes");
        doc[start..=gt].to_string()
    };

    // 1. The exiftool shape: the rectangle in property-element form in a
    //    Description of its own, the settings in Lightroom's attribute form.
    let apart = lr_doc("").replace(
        " <rdf:Description rdf:about=\"\"\n    xmlns:crs=",
        " <rdf:Description rdf:about=\"\"\n    xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\">\n\
             \x20  <tiff:ImageWidth>9504</tiff:ImageWidth>\n\
             \x20  <tiff:ImageLength>6336</tiff:ImageLength>\n\
             \x20 </rdf:Description>\n\
             \x20 <rdf:Description rdf:about=\"\"\n    xmlns:crs=",
    );
    assert!(apart.contains("<tiff:ImageWidth>9504</tiff:ImageWidth>"), "exiftool-shaped: {apart}");
    assert_eq!(declared_orientation(&apart), None);
    let out = turned(&apart);
    assert!(!out.doc.contains("tiff:Orientation"), "nothing the reader cannot see: {}", out.doc);
    assert_eq!(declared_orientation(&out.doc), None);
    assert!(out.doc.contains("<tiff:ImageWidth>9504</tiff:ImageWidth>"), "{}", out.doc);
    assert!(
        out.notes.iter().any(|n| n.contains("where a merge cannot rewrite it") && n.contains("(6)")),
        "the loss is named, with the turn that was not written: {:?}",
        out.notes
    );

    // 2. The frame on the crs Description, the prefix bound on an ancestor.
    let above = in_frame(&lr_doc(""), 9504, 6336).replace(
        "xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">",
        "xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"\n  xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\">",
    );
    assert_eq!(above.matches("xmlns:tiff=").count(), 1, "{above}");
    let out = turned(&above).doc;
    let tag = crs_tag(&out);
    assert_eq!(tag.matches("tiff:Orientation=\"6\"").count(), 1, "one correction: {tag}");
    assert_eq!(tag.matches("xmlns:tiff=").count(), 1, "bound where it is used: {tag}");
    assert_eq!(declared_orientation(&out), Some(O::Rotate90), "{out}");

    // 3. The Lightroom shape: the crs Description binds the prefix itself.
    let bound = in_frame(&lr_doc(""), 9504, 6336).replace(
        "tiff:ImageWidth",
        "xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\"\n   tiff:ImageWidth",
    );
    let out = turned(&bound).doc;
    assert_eq!(out.matches("xmlns:tiff=").count(), 1, "bound once: {out}");
    assert_eq!(out.matches("tiff:Orientation=\"6\"").count(), 1, "{out}");
    assert_eq!(declared_orientation(&out), Some(O::Rotate90), "{out}");
}

/// The shape 154 of the reference library's 174 sidecars actually have: a
/// `tiff:Orientation` and NO `tiff:ImageWidth`/`tiff:ImageLength` at all,
/// 17 of them over a portrait capture and carrying mask geometry.
///
/// The declaration alone is enough to place the geometry — `orient_point`
/// is aspect-free, and only a brush's radii need the rectangle — so the
/// reader turns on it, and the writer un-turns on it. Before W14 the
/// reader required the rectangle too, so those masks stayed in the sensor
/// frame while the render delivered the turned one (a quarter turn off),
/// and the merge wrote DISPLAY-frame numbers back under a declaration
/// saying they were sensor-frame.
///
/// MUTATION THIS CATCHES: gate the `orient_recipe_coords` call on
/// `frame.is_some()` again and the box comes back where the file put it,
/// i.e. on the wrong axis of the delivered frame.
#[test]
fn an_orientation_declared_without_a_rectangle_still_places_the_mask() {
    use rawler::Orientation as O;
    let doc = lr_doc(&lr_correction(
        "Mask 8",
        "",
        &lr_radial_at("0.100000", "0.200000", "0.300000", "0.600000", "0"),
    ))
    .replace("crs:Version=\"15.5.1\"", "tiff:Orientation=\"8\"\n   crs:Version=\"15.5.1\"");
    assert_eq!(declared_orientation(&doc), Some(O::Rotate270));
    assert!(FrameAspect::from_xmp(&doc).is_none(), "this document declares no rectangle");
    let r = xmp_to_recipe(&doc);
    let MaskGeometry::Radial { top, left, bottom, right, .. } = r.masks[0].mask else {
        panic!("a radial");
    };
    // `Rotate270` maps (u, v) -> (v, 1 - u), so the stored box's x range
    // 0.20..0.60 becomes the y range 0.40..0.80 and its y range 0.10..0.30
    // becomes the x range 0.10..0.30.
    assert!((left - 0.10).abs() < 1e-6 && (right - 0.30).abs() < 1e-6, "x {left}..{right}");
    assert!((top - 0.40).abs() < 1e-6 && (bottom - 0.80).abs() < 1e-6, "y {top}..{bottom}");

    // …and the writer un-turns it against the photograph's own rectangle,
    // which is the only one there is. The numbers that leave are the
    // file's, not the display frame's, and the base's own declaration is
    // untouched because it already says what the geometry now means.
    let out = merge_recipe_into_xmp_in_frame(
        &doc,
        &r,
        FrameAspect::from_size_turned(9504.0, 6336.0, O::Rotate270),
    )
    .expect("mergeable")
    .doc;
    assert_eq!(out.matches("tiff:Orientation").count(), 1, "{out}");
    assert!(out.contains("tiff:Orientation=\"8\""), "{out}");
    assert!(out.contains("crs:Left=\"0.2"), "the stored box is source-frame again: {out}");
    // The round trip is the identity on the engine's own numbers.
    let back = xmp_to_recipe(&out);
    let MaskGeometry::Radial { top: t1, left: l1, bottom: b1, right: r1, .. } =
        back.masks[0].mask
    else {
        panic!("a radial");
    };
    assert!(
        (l1 - left).abs() < 1e-5
            && (r1 - right).abs() < 1e-5
            && (t1 - top).abs() < 1e-5
            && (b1 - bottom).abs() < 1e-5,
        "the round trip moved the box: {l1}..{r1} x {t1}..{b1}"
    );
}

/// The REAL probe sidecars, when they are on the machine. Twelve controlled
/// Lightroom exports live at
/// `lr-experiment/` in the R25 materials ledger, outside the tree; point
/// `AUTOSHADE_LR_PROBE_FIXTURES` at that directory and this walks every
/// `.xmp` in it (and its `probe*/` subdirectories), asserting that every
/// radial imports and round-trips its corners byte-for-byte.
///
/// SILENTLY SKIPPED when the variable is unset — the files are the user's
/// photographs' metadata and are deliberately not in the repository, so
/// this cannot be a CI gate. The synthetic fixtures above carry the same
/// numbers; this is what proves the transcription.
#[test]
fn the_probe_sidecars_decode_to_their_measured_ellipses() {
    let Some(dir) = crate::config::live_env("AUTOSHADE_LR_PROBE_FIXTURES") else { return };
    let mut checked = 0usize;
    let mut roots = vec![std::path::PathBuf::from(dir)];
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    while let Some(root) = roots.pop() {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                roots.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xmp")) {
                files.push(p);
            }
        }
    }
    files.sort();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        if !text.contains("Mask/CircularGradient") {
            continue;
        }
        let frame = FrameAspect::from_xmp(&text);
        assert!(frame.is_some(), "{}: a Lightroom sidecar declares its frame", path.display());
        let r = xmp_to_recipe(&text);
        assert!(!r.masks.is_empty(), "{}: its radial must import", path.display());
        let out = recipe_to_xmp_in_frame(&r, frame).0;
        let read = |doc: &str, key: &str| -> Option<String> {
            let name = format!("crs:{key}=\"");
            let at = doc.find(&name)? + name.len();
            Some(doc[at..][..doc[at..].find('"')?].to_string())
        };
        // Every corner the file wrote comes back BYTE-identical — the
        // decode/encode pair is an algebraic inverse and `lr_num` is
        // Lightroom's own spelling.
        for key in ["Top", "Left", "Bottom", "Right"] {
            let Some(want) = read(&text, key) else { continue };
            assert_eq!(
                read(&out, key).as_deref(),
                Some(want.as_str()),
                "{}: crs:{key} did not survive the round trip",
                path.display()
            );
        }
        // The angle to 10⁻⁴ °, for the `f32` reason the synthetic
        // round-trip test spells out.
        if let (Some(w), Some(g)) = (read(&text, "Angle"), read(&out, "Angle")) {
            let (w, g): (f64, f64) = (w.parse().unwrap(), g.parse().unwrap());
            assert!((w - g).abs() < 1e-4, "{}: crs:Angle {g} vs {w}", path.display());
        }
        // R27: the CROP block travels the same road, and every one of
        // these files carries `crs:HasCrop="False"` — so the crop codec
        // must leave them exactly that, not invent a full-frame carrier.
        for key in ["HasCrop", "CropTop", "CropLeft", "CropBottom", "CropRight", "CropAngle"] {
            assert_eq!(
                read(&out, key),
                read(&text, key),
                "{}: crs:{key} did not survive the round trip",
                path.display()
            );
        }
        // …and the frame this writer declares is the frame the file
        // declared (A8), which is what makes the round trip re-readable.
        // Both sides are read through the RESOLVED frame scope (R29-2), so
        // this compares what the reader will actually see, not a
        // first-occurrence sweep of two whole documents.
        for key in ["tiff:ImageWidth", "tiff:ImageLength", "tiff:Orientation"] {
            let want = FrameScope::resolve(&text).declared_number(key);
            let got = FrameScope::resolve(&out).declared_number(key);
            assert_eq!(want, got, "{}: {key} {got:?} vs {want:?}", path.display());
        }
        checked += 1;
    }
    assert!(checked > 0, "AUTOSHADE_LR_PROBE_FIXTURES held no radial sidecar");
    eprintln!("AUTOSHADE_LR_PROBE_FIXTURES: {checked} radial sidecar(s) round-tripped");
}

/// R25 P5: both rotation verdicts CARRY the angle, so a disclosure can
/// say how much tilt it set aside instead of only that some existed.
/// `0` is the payload's word for "no angle to name" — an unreadable
/// `crs:Angle`, or a tilt that rounds away — and the prose channels fall
/// back to their plain phrasing on it.
#[test]
fn the_rotation_loss_names_the_angle() {
    let radial = |angle: f32| MaskGeometry::Radial {
        top: 0.3, left: 0.35, bottom: 0.7, right: 0.65,
        feather: 0.5, roundness: 0.0, flipped: false, angle,
        midpoint: 50.0, mask_version: 2,
    };
    let with = |angle: f32| EditRecipe {
        masks: vec![LocalAdjustment {
            mask: radial(angle),
            name: "tilted".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert_eq!(
        mask_export_losses(&with(37.412506)),
        vec![MaskLoss { name: "tilted".into(), reason: MaskLossReason::Rotation(37) }],
        "the export verdict names the engine's own angle"
    );
    assert_eq!(
        mask_export_losses(&with(-12.0)),
        vec![MaskLoss { name: "tilted".into(), reason: MaskLossReason::Rotation(-12) }],
        "including its sign"
    );
    assert_eq!(
        mask_export_losses(&with(0.4)),
        vec![MaskLoss { name: "tilted".into(), reason: MaskLossReason::Rotation(0) }],
        "a tilt under half a degree is still a loss, with no angle worth naming"
    );

    // The import twin, on the sidecar's own value — the measured negative
    // end of the reference library's range.
    let imported = |angle: &str| {
        import_losses(&lr_doc(&lr_correction("Radial 1", "", &lr_radial(angle, "0"))))
    };
    assert_eq!(
        imported("-43.945287"),
        vec![MaskImportLoss {
            name: "Radial 1".into(),
            reason: MaskImportReason::Rotation(-44)
        }],
        "the import verdict names Lightroom's angle"
    );
    assert_eq!(
        imported("oblique"),
        vec![MaskImportLoss {
            name: "Radial 1".into(),
            reason: MaskImportReason::Rotation(0)
        }],
        "an unreadable angle still counts as rotated — we cannot say it is zero"
    );
    assert!(imported("0").is_empty(), "and an unrotated radial loses nothing at all");
}

/// FORENSIC CONCLUSION, REVISED IN R25 P9 — read the revision, it is the
/// interesting part.
///
/// R24 observed that the reference sidecars carry `crs:Flipped="true"`
/// BESIDE `crs:MaskInverted="false"` on the same component, concluded the
/// two were INDEPENDENT (different concepts, both real), and this test
/// pinned all four combinations importing to distinct flags. The
/// observation was right and is unchanged; the CONCLUSION was wrong, and
/// what falsified it was sample size. R24 saw one cell of a 2×2. The R25
/// census walked the user's whole library — 201 radials — and found
/// `Flipped` and `MaskInverted` PERFECTLY ANTI-CORRELATED: 155 `(true,
/// false)`, 46 `(false, true)`, and ZERO of either matching pair. The two
/// M-B batches agree on the raw bytes and are not in conflict: "the fields
/// are not mirror copies of each other" (R24) and "their values are always
/// opposite" (R25) are both true of the same files. Only the reading
/// "therefore they mean different things" does not survive.
///
/// So Lightroom writes ONE inversion bit TWICE. This engine has TWO flags
/// composed by XOR (`Radial::flipped` in `mask_weight`, `inverted` in the
/// weight loop, render.rs), and importing both halves of Lightroom's
/// redundant pair XORed a value with its own complement: the net came out
/// `true` for EVERY imported Lightroom radial whatever the file said.
/// Measured on `P34` against the real Lightroom export, tone-matched
/// RMS 0.1099 → 0.0751 (blue 0.1901 → 0.0869) once the flip was dropped
/// (E1-verdict §6 defect 2).
///
/// MUTATION THIS CATCHES: putting `crs:Flipped` back into the geometry
/// flag re-inverts every Lightroom radial (rows 1 and 2 below), and
/// writing our own `flipped` straight back out re-emits a pair Lightroom
/// never writes (the last assertion).
#[test]
fn lightroom_spells_one_inversion_bit_twice() {
    // The two pairs Lightroom actually writes, then the two it never does.
    // For the observed pairs the net is `MaskInverted`; for the impossible
    // ones the tie is broken in favour of `MaskInverted`, which is the
    // attribute this reader trusts (see the importer's comment).
    for (flipped, inverted, net) in [
        (true, false, false), // 155 of 201 in the library
        (false, true, true),  //  46 of 201
        (true, true, true),   //   0 of 201 — resolved, not guessed
        (false, false, false), //  0 of 201
    ] {
        let comp = lr_radial("0", "0")
            .replace("crs:Flipped=\"true\"", &format!("crs:Flipped=\"{flipped}\""))
            .replace("crs:MaskInverted=\"false\"", &format!("crs:MaskInverted=\"{inverted}\""));
        let doc = lr_doc(&lr_correction("Radial 1", "", &comp));
        let r = xmp_to_recipe(&doc);
        let MaskGeometry::Radial { flipped: got_f, .. } = r.masks[0].mask else {
            panic!("expected a radial, got {:?}", r.masks[0].mask);
        };
        assert!(
            !got_f,
            "Flipped={flipped} Inverted={inverted}: crs:Flipped must not reach the render flag"
        );
        assert_eq!(
            r.masks[0].inverted, inverted,
            "Flipped={flipped} Inverted={inverted}: MaskInverted is the inversion"
        );
        assert_eq!(
            lr_net_inverted(&r.masks[0]),
            net,
            "Flipped={flipped} Inverted={inverted}: net inversion"
        );
        // …and OUR writer re-emits the pair Lightroom would have written
        // for that net, so the two rows Lightroom really uses round-trip
        // byte-for-byte and the two it never writes are normalised onto
        // the nearest row it does.
        let xmp = recipe_to_xmp(&r);
        assert!(
            xmp.contains(&format!("crs:MaskInverted=\"{net}\"")),
            "Flipped={flipped} Inverted={inverted}: MaskInverted must carry the net"
        );
        assert!(
            xmp.contains(&format!("crs:Flipped=\"{}\"", !net)),
            "Flipped={flipped} Inverted={inverted}: crs:Flipped is its complement"
        );
        assert_eq!(
            lr_net_inverted(&xmp_to_recipe(&xmp).masks[0]),
            net,
            "Flipped={flipped} Inverted={inverted}: the net survives the round trip"
        );
    }
}

/// R25 P9, the other direction: a mask THIS APP flipped (the GUI's Flip
/// checkbox — `flipped: true`, `inverted: false`) used to export as
/// `crs:Flipped="false" crs:MaskInverted="false"`, a combination Lightroom
/// never writes and reads as NOT inverted. The flip was dropped at the
/// border, silently, in the one direction the user cannot check from
/// inside this app.
///
/// MUTATION THIS CATCHES: any writer that copies `flipped` into
/// `crs:Flipped` instead of deriving both attributes from the net.
#[test]
fn our_own_flip_leaves_as_lightrooms_own_inversion() {
    for (flipped, inverted) in [(true, false), (false, true), (true, true), (false, false)] {
        let m = LocalAdjustment {
            mask: MaskGeometry::Radial {
                top: 0.3, left: 0.35, bottom: 0.7, right: 0.65,
                feather: 0.5, roundness: 0.0, flipped, angle: 0.0,
                midpoint: 50.0, mask_version: 2,
            },
            inverted,
            exposure_ev: -0.5,
            ..Default::default()
        };
        let net = flipped ^ inverted;
        let xmp = recipe_to_xmp(&EditRecipe { masks: vec![m], ..Default::default() });
        // Both attributes read off the SAME component tag — the pair is
        // the claim, and two whole-document `contains` could each be
        // satisfied by a different component.
        let p = Scope::new(xmp.as_str())
            .find_value_at("What", "Mask/CircularGradient")
            .expect("the radial must be emitted");
        let (s, e, _) = next_xml_tag(&xmp, p).expect("its component tag");
        let tag = Tag::new(&xmp[s..=e]);
        assert_eq!(
            tag.crs_str("MaskInverted").as_deref(),
            Some(if net { "true" } else { "false" }),
            "flipped={flipped} inverted={inverted}: the net must reach crs:MaskInverted"
        );
        assert_eq!(
            tag.crs_str("Flipped").as_deref(),
            Some(if net { "false" } else { "true" }),
            "flipped={flipped} inverted={inverted}: and its complement crs:Flipped — a \
                 matching pair is one Lightroom never writes, and it is what makes this \
                 projection safe under BOTH readings of which attribute Lightroom consults"
        );
        assert_eq!(
            lr_net_inverted(&xmp_to_recipe(&xmp).masks[0]),
            net,
            "flipped={flipped} inverted={inverted}: the rendered result must survive"
        );
    }
}

/// The other half of §0: `crs:MaskBlendMode` is on every component
/// Lightroom writes, and the import refused it unless WE had written the
/// file. Its default value is the plain composition the engine already
/// does, so accepting it costs nothing — and a lossless import must
/// report NO loss, or the disclosure cries wolf on every photo.
#[test]
fn a_lightroom_gradient_with_blend_mode_zero_imports_losslessly() {
    let doc = lr_doc(&lr_correction("Gradient 1", "", &lr_gradient("0")));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "a plain Lightroom gradient must import: {:?}", r.masks);
    assert!(import_losses(&doc).is_empty(), "a faithful import says nothing");
    assert_eq!(unsupported_corrections(&doc), 0);
    // …and a mode we cannot reproduce is a NOTE on an imported mask, not
    // a refusal: the shape is still exactly what the file draws.
    // (`lr_gradient` pairs the mode with `MaskValue="1"`; that pair does
    // not occur in the wild — see the next test, which uses the one that
    // does — but it isolates the blend-mode arm on its own.)
    let subtract = lr_doc(&lr_correction("Gradient 1", "", &lr_gradient("1")));
    let r2 = xmp_to_recipe(&subtract);
    assert_eq!(r2.masks.len(), 1, "a Subtract component still has a shape");
    assert_eq!(
        import_losses(&subtract),
        vec![MaskImportLoss {
            name: "Gradient 1".into(),
            reason: MaskImportReason::BlendMode
        }]
    );
}

/// v0.31.1: Lightroom's SUBTRACT is the PAIR `crs:MaskBlendMode="1"` +
/// `crs:MaskValue="0"`, and the second half is an ENCODING, not an opacity.
///
/// EVIDENCE (complete census of the GitHub-code-search-indexed population
/// of `.xmp` files containing `crs:MaskBlendMode` — 157 files, 479
/// attribute instances, verified twice, by regex and by
/// `xml.etree.ElementTree`, 0 parse failures, 2026-08-18):
/// `MaskBlendMode="1"` co-occurs with `MaskValue="0"` in 26 of 26
/// instances, and `MaskBlendMode="1"` with `MaskValue="1"` in 0 of 479.
/// The attribute never sits on the `crs:What="Correction"` element — it is
/// always a per-component value, which is where this reader looks.
///
/// The importer read that zero as "muted", refused the component, and the
/// geometry arm turned the refusal into `OutOfModel` — the user's whole
/// correction, thrown away to avoid a composition we could simply have
/// disclosed. Now the base shape imports and `BlendMode` names what did
/// not. The zero is never multiplied into anything; strength comes from
/// `crs:CorrectionAmount`, which this test also pins.
///
/// MUTATION THIS CATCHES: reading `MaskValue` as an opacity (the mask
/// arrives at strength 0), and dropping the `subtracted` guard (the whole
/// correction disappears again).
#[test]
fn a_real_lightroom_subtract_component_keeps_its_geometry() {
    // The real pair, on a component this reader has a model for.
    let subtract = lr_radial("0", "1").replace("crs:MaskValue=\"1\"", "crs:MaskValue=\"0\"");
    let doc = lr_doc(&lr_correction("Radial 1", "", &subtract));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "the shape the file draws must survive: {:?}", r.masks);
    assert_eq!(unsupported_corrections(&doc), 0, "and the correction is not counted lost");
    assert_eq!(
        import_losses(&doc),
        vec![MaskImportLoss {
            name: "Radial 1".into(),
            reason: MaskImportReason::BlendMode
        }],
        "exactly one note: the composition, not the geometry"
    );
    // The geometry is the file's own, and the zero did NOT become a
    // strength: `CorrectionAmount="1"` is still the master opacity.
    let MaskGeometry::Radial { top, left, .. } = r.masks[0].mask else {
        panic!("expected a radial, got {:?}", r.masks[0].mask);
    };
    // The file's own `(0.114928, 0.590368)`, VERBATIM: the frame affine is
    // the identity since the 2026-08-19 `LR_MASK_FRAME_SCALE = 1.0` ruling
    // (see `a_lightroom_radial_with_angle_imports`).
    assert!(
        (top as f64 - 0.114928).abs() < 1e-7 && (left as f64 - 0.590368).abs() < 1e-7,
        "the real coordinates arrived: {top} {left}"
    );
    assert_eq!(r.masks[0].amount, 1.0, "MaskValue=0 is an encoding, never a pre-multiplier");
    assert_eq!(r.masks[0].contrast, 43.0, "and the sliders are untouched by it");

    // The guard is a PAIR. A zero MaskValue with the DEFAULT blend mode is
    // the muted component it always was, and still refuses.
    let muted = lr_radial("0", "0").replace("crs:MaskValue=\"1\"", "crs:MaskValue=\"0\"");
    let muted_doc = lr_doc(&lr_correction("Radial 1", "", &muted));
    assert!(xmp_to_recipe(&muted_doc).masks.is_empty(), "a muted component still refuses");
    assert_eq!(unsupported_corrections(&muted_doc), 1);

    // …and so does a genuinely PARTIAL value, blend mode or not — that is
    // coverage we can read and have no model for. (0.662178 is the one
    // non-0/1 MaskValue in the whole 479-instance census.)
    for blend in ["0", "1"] {
        let partial = lr_radial("0", blend)
            .replace("crs:MaskValue=\"1\"", "crs:MaskValue=\"0.662178\"");
        let partial_doc = lr_doc(&lr_correction("Radial 1", "", &partial));
        assert!(
            xmp_to_recipe(&partial_doc).masks.is_empty(),
            "MaskBlendMode={blend}: a partial MaskValue is not the subtract encoding"
        );
    }
}

/// v0.31.1: `crs:Roundness` is Lightroom's ±100 integer slider, so a user
/// who moved it no longer loses the mask.
///
/// EVIDENCE (direct observation of the harvested real-sidecar corpus,
/// 2026-08-18): all 24 `Mask/CircularGradient` components write Roundness
/// as a bare signed integer, every one at its default `0`, beside
/// `Feather="+100"` and `Midpoint="+50"` — integers on a 0..100-style
/// footing, not 0..1 reals. The importer's old `(0.0..=1.0)` gate was the
/// "bbox aspect ratio" reading of the field, and being a GEOMETRY check it
/// refused the entire correction.
///
/// The value is CARRIED, not converted (`mask_weight` never reads it), so
/// the ambiguous `1` needs no ruling: whatever scale it was written on, it
/// is written back as `1`. That is the difference from `feather`, which is
/// rendered and therefore must guess.
#[test]
fn a_lightroom_roundness_slider_is_carried_not_refused() {
    let with = |v: &str| {
        lr_doc(&lr_correction(
            "Radial 1",
            "",
            &lr_radial("0", "0").replace("crs:Roundness=\"0\"", &format!("crs:Roundness=\"{v}\"")),
        ))
    };
    // Both signs of the real slider, the ambiguous 1, and a value only our
    // own legacy writer could have produced.
    for (text, want) in [("-30", -30.0), ("+45", 45.0), ("1", 1.0), ("0.25", 0.25)] {
        let doc = with(text);
        let r = xmp_to_recipe(&doc);
        assert_eq!(r.masks.len(), 1, "Roundness={text} must not cost the mask: {:?}", r.masks);
        assert!(import_losses(&doc).is_empty(), "Roundness={text}: carrying it loses nothing");
        let MaskGeometry::Radial { roundness, .. } = r.masks[0].mask else {
            panic!("expected a radial, got {:?}", r.masks[0].mask);
        };
        assert_eq!(roundness, want, "Roundness={text}: carried verbatim, never rescaled");
        // …and it goes back out as the same number, through the clamp.
        let back = xmp_to_recipe(&recipe_to_xmp(&r));
        let MaskGeometry::Radial { roundness: round2, .. } = back.masks[0].mask else {
            panic!("expected a radial, got {:?}", back.masks[0].mask);
        };
        assert_eq!(round2, want, "Roundness={text} did not survive our own writer");
    }
    // The gate still has ends: past the slider's own range is unreadable
    // geometry, and that IS a refusal.
    let wild = with("101");
    assert!(xmp_to_recipe(&wild).masks.is_empty(), "101 is off the Lightroom slider");
    assert_eq!(unsupported_corrections(&wild), 1);
}

/// A `crs:Local*` slider this engine has no model for used to cost the
/// user the whole mask. It is a knob, not a coverage change: the shape
/// and the fifteen sliders we DO model are still exactly the file's.
#[test]
fn a_nonzero_inert_local_no_longer_drops_the_whole_correction() {
    let doc = lr_doc(
        &lr_correction("Radial 1", "", &lr_radial("0", "0"))
            .replace("crs:LocalDefringe=\"0\"", "crs:LocalDefringe=\"0.3\""),
    );
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "the mask imports: {:?}", r.masks);
    assert_eq!(r.masks[0].contrast, 43.0, "the modelled sliders came through");
    assert_eq!(
        import_losses(&doc),
        vec![MaskImportLoss {
            name: "Radial 1".into(),
            reason: MaskImportReason::InertLocal("LocalDefringe")
        }],
        "the slider that did not come through is named"
    );
    assert_eq!(unsupported_corrections(&doc), 0);
}

/// A `crs:Local*` attribute this build has never seen: same rule, and the
/// loss carries the CORRECTION's name so the sentence is actionable.
/// Also pins the two notes stacking on one correction.
#[test]
fn unknown_local_key_names_itself() {
    let doc = lr_doc(&lr_correction(
        "Sky",
        "       crs:LocalWhatsit=\"0.5\"\n",
        &lr_radial("0", "0"),
    ))
    .replace("crs:LocalCurveRefineSaturation=\"100\"", "crs:LocalCurveRefineSaturation=\"80\"");
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "an unknown knob is not a reason to lose the mask");
    let losses = import_losses(&doc);
    assert_eq!(losses.len(), 2, "both notes are raised: {losses:?}");
    assert!(losses.iter().all(|l| l.name == "Sky"), "each names the correction: {losses:?}");
    assert!(
        losses.iter().any(|l| l.reason == MaskImportReason::UnknownLocalKey),
        "{losses:?}"
    );
    assert!(
        losses.iter().any(|l| l.reason == MaskImportReason::CurveRefineSaturation),
        "{losses:?}"
    );
}

/// R25 P1 round-end. `crs:LocalCorrectedDepth`, `crs:LocalInputDigest`
/// and `crs:LocalInputDigestVersion` are Lightroom's own BOOKKEEPING —
/// a numeric flag and a recompute ledger (32-hex digest + schema version)
/// — and they rode into `UnknownLocalKey`, whose label reads "unmodelled
/// slider". That sentence was wrong about the thing AND wrong about how
/// often: 12 notes across the reference library's 31 importable
/// corrections, on files whose sliders all came through intact.
///
/// Knowing a key and not modelling it is the honest answer for all three.
/// The numeric one keeps the inert-key law all the same — silent at its
/// observed 0, NAMED at anything else — while the two string keys stay out
/// of `INERT_LOCAL`, because `optional_number_is` cannot parse a hex
/// digest and would raise a false note on every file that carries one.
#[test]
fn lightroom_bookkeeping_keys_are_known_not_unmodelled_sliders() {
    // The shapes are Lightroom's, the digest is a neutral test value: a
    // real digest is a hash OF the user's own file (fixture policy).
    let ledger = "       crs:LocalCorrectedDepth=\"0\"\n\
                      \x20      crs:LocalInputDigest=\"0000000000000000000000000000002A\"\n\
                      \x20      crs:LocalInputDigestVersion=\"1\"\n";
    let doc = lr_doc(&lr_correction("Sky", ledger, &lr_radial("0", "0")));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "premise: the correction imports at all");
    assert!(
        import_losses(&doc).is_empty(),
        "Lightroom's own bookkeeping is not a loss: {:?}",
        import_losses(&doc)
    );

    // The inert-key law still holds for the numeric one. MUTATION THIS
    // CATCHES: adding it to `KNOWN_LOCAL` and NOT to `INERT_LOCAL` makes
    // a non-zero value silent, which is the opposite failure.
    let moved = doc.replace("crs:LocalCorrectedDepth=\"0\"", "crs:LocalCorrectedDepth=\"0.5\"");
    assert_eq!(
        import_losses(&moved),
        vec![MaskImportLoss {
            name: "Sky".into(),
            reason: MaskImportReason::InertLocal("LocalCorrectedDepth")
        }],
        "a bookkeeping flag off its observed value is named like any other inert key"
    );
    assert_eq!(xmp_to_recipe(&moved).masks.len(), 1, "and it still costs no mask");

    // The two STRING keys can never be read as numbers — if either were
    // in `INERT_LOCAL`, this document would raise a note for a value that
    // is exactly what Lightroom writes.
    assert!(
        import_losses(&doc.replace(
            "crs:LocalInputDigestVersion=\"1\"",
            "crs:LocalInputDigestVersion=\"2\""
        ))
        .is_empty(),
        "a digest ledger is not a slider at any value"
    );
}

/// THE TRAP OF THIS BATCH (data-corruption class). Once a lossy sidecar
/// imports, `r.masks.is_empty()` stops meaning "the user has not touched
/// these" — and the merge used that emptiness to decide whether to keep
/// the base's own mask block. Left alone, an ordinary Ctrl+S would have
/// written our DEGRADED reading (rotation read as 0, blend mode ignored,
/// `crs:Midpoint` / `crs:Version` not even read) over the user's own
/// Lightroom block, silently.
///
/// The whole round trip the app really takes is exercised, not just the
/// merge: import → serde_json → back (recipe.json is a file, and f32 that
/// does not survive the text round trip would make the equality fail on
/// the user's second launch, not in a unit test) → merge → the base's
/// mask block must come out byte-for-byte.
#[test]
fn preserve_masks_survives_a_lossy_import_the_user_did_not_touch() {
    let doc = lr_doc(&format!(
        "{}{}",
        lr_correction("Radial 1", "", &lr_radial("37.412506", "0")),
        lr_correction("Gradient 1", "", &lr_gradient("1")),
    ));
    let imported = xmp_to_recipe(&doc);
    assert_eq!(imported.masks.len(), 2, "premise: the masks really did import");
    assert_eq!(import_losses(&doc).len(), 2, "premise: the import really was lossy");

    // recipe.json in the middle, exactly as the app stores it.
    let json = serde_json::to_string(&imported).expect("serialise");
    let reloaded: EditRecipe = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(
        reloaded.masks, imported.masks,
        "an f32 that does not survive recipe.json would silently arm the overwrite"
    );

    let start = doc.find("<crs:MaskGroupBasedCorrections>").expect("block");
    let end = doc.find("</crs:MaskGroupBasedCorrections>").expect("block close")
        + "</crs:MaskGroupBasedCorrections>".len();
    let original = &doc[start..end];
    let out = merge_recipe_into_xmp(&doc, &reloaded).expect("mergeable");
    assert!(
        out.doc.contains(original),
        "the user's own mask block must survive an untouched save VERBATIM:\n{}",
        out.doc
    );
    assert!(
        out.doc.contains("crs:Angle=\"37.412506\"") && out.doc.contains("crs:Midpoint=\"50\""),
        "the parts we cannot express are exactly the parts this rule protects"
    );
    assert!(
        out.notes.is_empty(),
        "nothing was replaced, so nothing to disclose: {:?}",
        out.notes
    );
    // Our own projection is NOT prepended beside the base's block — one
    // document, two mask groups was the other way to get this wrong.
    assert_eq!(
        out.doc.matches("<crs:MaskGroupBasedCorrections>").count(),
        1,
        "exactly one mask group in the output"
    );
}

/// A `recipe.json` in the shape v0.30 wrote them: today's serialisation
/// MINUS `schema_era` and minus every field R25 added a `crs:` key for.
/// Built by deletion rather than by hand so the fixture cannot quietly
/// stop being a subset of what the app really writes — and read back
/// through the real serde path, because the whole point is what the FIELD
/// DEFAULTS do with an absent key.
fn as_v0_30_recipe(r: &EditRecipe) -> EditRecipe {
    let mut v = serde_json::to_value(r).expect("serialise");
    let obj = v.as_object_mut().expect("a recipe is an object");
    assert!(obj.remove("schema_era").is_some(), "the era stamp must have been there to remove");
    for (name, _) in era_attr_keys(1) {
        assert!(obj.remove(name).is_some(), "{name} is a recipe field");
    }
    // v1.5.0's fields are skipped at their defaults, so one is there to
    // remove only when the recipe moved it. By CONTROL, not by attribute:
    // the point colours are an element and have no attribute key.
    for name in crate::recipe::V150_CONTROLS {
        obj.remove(name);
    }
    serde_json::from_value(v).expect("deserialise")
}

/// The same deletion for a `recipe.json` in the shape v1.4 wrote: stamped
/// era 1, and without the keys v1.5.0 added.
fn as_v1_4_recipe(r: &EditRecipe) -> EditRecipe {
    let mut v = serde_json::to_value(r).expect("serialise");
    let obj = v.as_object_mut().expect("a recipe is an object");
    obj.insert("schema_era".to_string(), serde_json::json!(1));
    for name in crate::recipe::V150_CONTROLS {
        obj.remove(name);
    }
    serde_json::from_value(v).expect("deserialise")
}

/// A Lightroom sidecar carrying the R25 globals and no masks — the shape
/// the B2 / B3 keys actually arrive in (values from the reference
/// library: a negative Texture, the one signed decimal in the detail
/// block, a real post-crop vignette, a grain triple, and the de-fringe
/// block at Adobe's own defaults with one non-zero amount).
fn lr_globals_doc() -> String {
    "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Adobe XMP Core 5.6-c145\">\n\
         \x20<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         \x20 <rdf:Description rdf:about=\"\"\n\
         \x20   xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n\
         \x20   crs:Version=\"15.5.1\"\n\
         \x20   crs:ProcessVersion=\"15.4\"\n\
         \x20   crs:Exposure2012=\"+0.35\"\n\
         \x20   crs:Texture=\"-20\"\n\
         \x20   crs:SharpenRadius=\"+1.0\"\n\
         \x20   crs:SharpenDetail=\"25\"\n\
         \x20   crs:PostCropVignetteAmount=\"-17\"\n\
         \x20   crs:PostCropVignetteMidpoint=\"50\"\n\
         \x20   crs:GrainAmount=\"30\"\n\
         \x20   crs:GrainSize=\"25\"\n\
         \x20   crs:GrainFrequency=\"50\"\n\
         \x20   crs:DefringePurpleAmount=\"3\"\n\
         \x20   crs:DefringePurpleHueLo=\"30\"\n\
         \x20   crs:DefringePurpleHueHi=\"70\"\n\
         \x20   crs:DefringeGreenAmount=\"0\"\n\
         \x20   crs:DefringeGreenHueLo=\"40\"\n\
         \x20   crs:DefringeGreenHueHi=\"60\"\n\
         \x20   crs:HasSettings=\"True\"/>\n\
         \x20</rdf:RDF>\n\
         </x:xmpmeta>\n"
        .to_string()
}

/// **THE R25 P8 TRAP, mask half** (data-destruction class), and the exact
/// scenario measured on the reference library: P31 lost four
/// corrections and P51 lost eight, with an empty note list.
///
/// P1 made Lightroom's own masks import CLEANLY, and the merge's preserve
/// arm was keyed on a `MaskSummary::preserve_original` flag that only a
/// LOSSY import ever set (it is gone now; `defects > 0` is what it meant). A v0.30 `recipe.json` is maskless by construction (that
/// build could not import one), so the pair "clean base + maskless
/// recipe" answered "nothing to preserve", and an ordinary Ctrl+S stripped
/// the block and wrote nothing in its place.
///
/// MUTATION THIS CATCHES: put the old flag's condition —
/// `summary.defects > 0`, which is exactly when `preserve_original` was
/// raised — back into either the preserve or the note test, and this goes
/// red on both assertions at once: the block vanishes AND nothing says so.
#[test]
fn a_clean_lightroom_mask_block_survives_a_recipe_that_never_saw_it() {
    let doc = lr_doc(&format!(
        "{}{}",
        lr_correction("Radial 1", "", &lr_radial("0", "0")),
        lr_correction("Gradient 1", "", &lr_gradient("0")),
    ));
    let imported = xmp_to_recipe(&doc);
    assert_eq!(imported.masks.len(), 2, "premise: the masks import");
    assert!(
        import_losses(&doc).is_empty(),
        "premise: and they import CLEANLY — that is what broke the old flag: {:?}",
        import_losses(&doc)
    );

    // The v0.30 recipe.json beside that sidecar: no masks, no era stamp.
    let legacy = EditRecipe { masks: Vec::new(), ..as_v0_30_recipe(&imported) };
    assert_eq!(legacy.schema_era, 0, "an absent key is what makes it legacy");

    let start = doc.find("<crs:MaskGroupBasedCorrections>").expect("block");
    let end = doc.find("</crs:MaskGroupBasedCorrections>").expect("block close")
        + "</crs:MaskGroupBasedCorrections>".len();
    let original = &doc[start..end];
    let out = merge_recipe_into_xmp(&doc, &legacy).expect("mergeable");
    assert!(
        out.doc.contains(original),
        "the photographer's own mask block must survive VERBATIM:\n{}",
        out.doc
    );
    assert_eq!(
        out.doc.matches("<crs:MaskGroupBasedCorrections>").count(),
        1,
        "exactly one mask group in the output"
    );
    assert!(out.notes.is_empty(), "nothing was replaced: {:?}", out.notes);
}

/// The disclosure half of the same rule, on a base the importer
/// understands COMPLETELY. Before P8 this arm could not be reached at all
/// (the note was gated on that same lossy-import flag), so replacing perfectly
/// readable Lightroom corrections was a silent success — and the sentence
/// itself had to change, because "carries 0 thing(s) this build cannot
/// represent" is what the old wording says about a clean block.
#[test]
fn replacing_a_clean_mask_block_names_what_it_replaced() {
    let doc = lr_doc(&lr_correction("Radial 1", "", &lr_radial("0", "0")));
    assert!(import_losses(&doc).is_empty(), "premise: a clean base");
    let mut r = xmp_to_recipe(&doc);
    r.masks[0].exposure_ev = 1.25; // the user moved it: the develop is newest
    let out = merge_recipe_into_xmp(&doc, &r).expect("mergeable");
    assert_eq!(out.notes.len(), 1, "the replacement is disclosed: {:?}", out.notes);
    assert!(
        out.notes[0].contains("1 correction(s)")
            && !out.notes[0].contains("0 thing(s)")
            && out.notes[0].contains("1 edited mask(s)"),
        "the note counts what was there, not a defect count of zero: {}",
        out.notes[0]
    );
}

/// **THE R25 P8 TRAP, globals half** (data-destruction class): three of the
/// seven reference sidecars lost nine keys each to this, silently.
///
/// Owning a key means the merge STRIPS it and the writer puts ours back —
/// and the writer omits a slider at rest. A v0.30 `recipe.json` has no
/// field for any of the twenty-seven keys R25 added, so serde fills them
/// from `EditRecipe::default()` and the recipe "says" texture 0, no grain,
/// no radius. Stripping on that reading deleted `crs:Texture="-20"`, the
/// whole Grain block and the PostCrop / SharpenRadius keys out of the
/// photographer's Lightroom file on an ordinary Ctrl+S.
///
/// MUTATION THIS CATCHES: return an empty set from
/// `unspoken_attr_keys` (or drop its `schema_era` test) and every
/// value below goes to the writer's default.
#[test]
fn a_v0_30_recipe_does_not_strip_the_keys_it_never_had() {
    let doc = lr_globals_doc();
    let imported = xmp_to_recipe(&doc);
    assert_eq!(imported.texture, -20.0, "premise: the fixture really carries them");
    assert_eq!((imported.grain, imported.sharpen_radius), (30.0, 1.0));

    let legacy = as_v0_30_recipe(&imported);
    assert_eq!(legacy.schema_era, 0);
    assert_eq!(legacy.texture, 0.0, "premise: serde filled the absent field with the default");

    let out = merge_recipe_into_xmp(&doc, &legacy).expect("mergeable");
    for spelling in [
        "crs:Texture=\"-20\"",
        "crs:SharpenRadius=\"+1.0\"",
        "crs:SharpenDetail=\"25\"",
        "crs:PostCropVignetteAmount=\"-17\"",
        "crs:PostCropVignetteMidpoint=\"50\"",
        "crs:GrainAmount=\"30\"",
        "crs:GrainSize=\"25\"",
        "crs:GrainFrequency=\"50\"",
        "crs:DefringePurpleAmount=\"3\"",
    ] {
        assert!(out.doc.contains(spelling), "{spelling} was deleted from the user's file");
    }
    // Suppressing the strip WITHOUT suppressing the write is the other way
    // to get this wrong: one tag, two answers.
    for key in ["Texture", "GrainAmount", "DefringePurpleAmount", "DefringePurpleHueLo"] {
        assert_eq!(
            out.doc.matches(&format!("crs:{key}=")).count(),
            1,
            "crs:{key} must appear exactly once"
        );
    }
    assert_eq!(xmp_to_recipe(&out.doc).texture, -20.0, "…and it reads back as itself");
    assert!(out.notes.is_empty(), "nothing was lost, so nothing to disclose: {:?}", out.notes);
}

/// The CONTROL for the test above, and the reason the gate is an era stamp
/// rather than a new policy: a CURRENT-era recipe that says texture 0 is
/// STATING a value, and the merge must still publish it over the base's.
/// Whatever else this batch changed, it did not change what a save means.
#[test]
fn a_current_era_recipe_still_owns_every_key_it_states() {
    let doc = lr_globals_doc();
    let mut r = xmp_to_recipe(&doc);
    assert_eq!(r.schema_era, crate::recipe::SCHEMA_ERA, "an import is current-era");
    r.texture = 0.0;
    r.grain = 0.0;
    let out = merge_recipe_into_xmp(&doc, &r).expect("mergeable");
    assert!(out.doc.contains("crs:Texture=\"0\""), "the cleared slider publishes: {}", out.doc);
    assert!(!out.doc.contains("crs:GrainAmount="), "a zero grain is an omitted key");
    assert_eq!(xmp_to_recipe(&out.doc).texture, 0.0);
}

/// The gate is PER KEY, and this is the case that forces it: a legacy
/// recipe whose Texture the user has just dragged. Nothing ever re-stamps
/// a file's era, so a whole-recipe gate would mean a v0.30 photo could
/// never write Texture to its sidecar again — the same silent divergence
/// class this round is closing, re-introduced by the fix for it.
#[test]
fn an_edited_key_leaves_the_era_gate_even_on_a_legacy_recipe() {
    let doc = lr_globals_doc();
    let mut legacy = as_v0_30_recipe(&xmp_to_recipe(&doc));
    legacy.texture = 20.0; // the user moved THIS slider and nothing else
    let out = merge_recipe_into_xmp(&doc, &legacy).expect("mergeable");
    assert!(out.doc.contains("crs:Texture=\"+20\""), "the edit reaches the file: {}", out.doc);
    assert_eq!(out.doc.matches("crs:Texture=").count(), 1, "and only once");
    // Its untouched neighbours are still protected — the gate did not open
    // for the whole recipe.
    assert!(out.doc.contains("crs:GrainAmount=\"30\""), "the grain block stands");
}

/// The de-fringe six move as ONE block or not at all: the writer emits all
/// six unconditionally because a hue window with no amount beside it is a
/// shape no real document has, and a per-key gate that released three of
/// them would publish exactly that.
#[test]
fn the_era_gate_releases_the_de_fringe_block_whole() {
    let doc = lr_globals_doc();
    let mut legacy = as_v0_30_recipe(&xmp_to_recipe(&doc));
    legacy.defringe_purple = 5.0; // one key of the six
    let out = merge_recipe_into_xmp(&doc, &legacy).expect("mergeable");
    for key in [
        "DefringePurpleAmount",
        "DefringePurpleHueLo",
        "DefringePurpleHueHi",
        "DefringeGreenAmount",
        "DefringeGreenHueLo",
        "DefringeGreenHueHi",
    ] {
        assert_eq!(
            out.doc.matches(&format!("crs:{key}=")).count(),
            1,
            "crs:{key} must be written exactly once when the block moves"
        );
    }
    assert!(out.doc.contains("crs:DefringePurpleAmount=\"5\""), "{}", out.doc);
}

/// The era gate's universe for era 1, DERIVED and pinned: exactly the
/// twenty-seven attribute keys R25 gave this writer. The controls are
/// named per era in `recipe::SCHEMA_ERA_CONTROLS` and the spellings derived
/// from their registry rows; this asserts the derivation produces the list,
/// whatever tier those rows have since moved to.
#[test]
fn the_era_gate_is_the_twenty_seven_keys_r25_added() {
    let mut keys: Vec<&str> = era_attr_keys(1).into_iter().map(|(_, k)| k).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "AutoLateralCA",
            "ChromaticAberrationB",
            "ChromaticAberrationR",
            "ColorNoiseReduction",
            "ColorNoiseReductionDetail",
            "ColorNoiseReductionSmoothness",
            "DefringeGreenAmount",
            "DefringeGreenHueHi",
            "DefringeGreenHueLo",
            "DefringePurpleAmount",
            "DefringePurpleHueHi",
            "DefringePurpleHueLo",
            "GrainAmount",
            "GrainFrequency",
            "GrainSize",
            "LuminanceNoiseReductionContrast",
            "LuminanceNoiseReductionDetail",
            "PostCropVignetteAmount",
            "PostCropVignetteFeather",
            "PostCropVignetteHighlightContrast",
            "PostCropVignetteMidpoint",
            "PostCropVignetteRoundness",
            "PostCropVignetteStyle",
            "SharpenDetail",
            "SharpenEdgeMasking",
            "SharpenRadius",
            "Texture",
        ]
    );
    // Every one of them is a key this writer OWNS — a gated key the merge
    // never strips anyway would be a rule about nothing.
    let owned = owned_attr_keys();
    for k in &keys {
        assert!(owned.contains(&(*k).to_string()), "{k} is not an owned attribute");
    }
    // And the gate really is EMPTY for a current-era recipe: the ordinary
    // save path pays nothing and changes nothing.
    // A current-era recipe is gated on nothing it has SEEN. The one key it
    // is still silent about is the profile NAME, and for the other reason
    // `unspoken_attr_keys` now carries: an empty name is "not stated", so
    // an ordinary save leaves Lightroom's own profile alone.
    assert_eq!(
        unspoken_attr_keys(&EditRecipe::default()).into_iter().collect::<Vec<_>>(),
        vec!["CameraProfile"]
    );
    assert!(
        unspoken_attr_keys(&EditRecipe {
            camera_profile: "Adobe Standard".to_string(),
            ..Default::default()
        })
        .is_empty(),
        "a recipe that NAMES a profile speaks for the key and owns it"
    );
    let v150 = era_attr_keys(2).len();
    assert_eq!(
        unspoken_attr_keys(&EditRecipe { schema_era: 0, ..Default::default() }).len(),
        27 + v150,
        "an untouched v0.30 recipe suppresses all twenty-seven and every later era's"
    );
}

/// v1.5.0's era: the keys it added, DERIVED and pinned like R25's, and the
/// gate PER ERA — an untouched v1.4 (era-1) recipe suppresses exactly
/// these and not one R25 key, because it has held those all along.
///
/// MUTATION THIS CATCHES: `unspoken_attr_keys` gating from era 1
/// regardless of the stamp (an era-1 recipe would lose its R25 keys to the
/// gate — a Texture cleared in AutoShade would stop reaching the sidecar),
/// or a v1.5.0 control left off `recipe::V150_CONTROLS`.
#[test]
fn the_era_gate_names_the_keys_v1_5_0_added() {
    let mut keys: Vec<&str> = era_attr_keys(2).into_iter().map(|(_, k)| k).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "BlueHue",
            "BlueSaturation",
            "CameraProfile",
            "ConvertToGrayscale",
            "CropConstrainToWarp",
            "GrayMixerAqua",
            "GrayMixerBlue",
            "GrayMixerGreen",
            "GrayMixerMagenta",
            "GrayMixerOrange",
            "GrayMixerPurple",
            "GrayMixerRed",
            "GrayMixerYellow",
            "GreenHue",
            "GreenSaturation",
            // v1.5.0 F8.
            "HDREditMode",
            "HDRMaxValue",
            "LensProfileDistortionScale",
            "LensProfileVignettingScale",
            "ParametricDarks",
            "ParametricHighlightSplit",
            "ParametricHighlights",
            "ParametricLights",
            "ParametricMidtoneSplit",
            "ParametricShadowSplit",
            "ParametricShadows",
            "PerspectiveAspect",
            "PerspectiveHorizontal",
            "PerspectiveRotate",
            "PerspectiveScale",
            "PerspectiveUpright",
            "PerspectiveVertical",
            "PerspectiveX",
            "PerspectiveY",
            "RedHue",
            "RedSaturation",
            "SDRBlend",
            "SDRBrightness",
            "SDRClarity",
            "SDRContrast",
            "SDRHighlights",
            "SDRShadows",
            "SDRWhites",
            "ShadowTint",
        ]
    );
    let owned = owned_attr_keys();
    for k in &keys {
        assert!(owned.contains(&(*k).to_string()), "{k} is not an owned attribute");
    }
    let mut gated: Vec<&str> =
        unspoken_attr_keys(&EditRecipe { schema_era: 1, ..Default::default() }).into_iter().collect();
    gated.sort_unstable();
    assert_eq!(gated, keys, "an untouched v1.4 recipe is gated on v1.5.0's keys and nothing else");
    assert!(era_attr_keys(0).is_empty() && era_attr_keys(crate::recipe::SCHEMA_ERA + 1).is_empty());
}

/// A Lightroom sidecar with a real parametric curve — the block in the
/// shape Lightroom 9.4 writes it (all seven keys, signed regions, bare
/// splits) — beside a Basic-panel Texture.
fn lr_parametric_doc() -> String {
    "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Adobe XMP Core 5.6-c145\">\n\
         \x20<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         \x20 <rdf:Description rdf:about=\"\"\n\
         \x20   xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n\
         \x20   crs:Version=\"15.5.1\"\n\
         \x20   crs:ProcessVersion=\"15.4\"\n\
         \x20   crs:Texture=\"-20\"\n\
         \x20   crs:ParametricShadows=\"0\"\n\
         \x20   crs:ParametricDarks=\"+40\"\n\
         \x20   crs:ParametricLights=\"-15\"\n\
         \x20   crs:ParametricHighlights=\"0\"\n\
         \x20   crs:ParametricShadowSplit=\"25\"\n\
         \x20   crs:ParametricMidtoneSplit=\"50\"\n\
         \x20   crs:ParametricHighlightSplit=\"80\"\n\
         \x20   crs:HasSettings=\"True\"/>\n\
         \x20</rdf:RDF>\n\
         </x:xmpmeta>\n"
        .to_string()
}

/// v1.5.0: the parametric curve reads in as Lightroom states it, writes
/// back as the whole block Lightroom writes, and stays out of a sidecar
/// whose recipe never moved it.
///
/// MUTATIONS THIS CATCHES: the reader's split fallback to 0 (a document
/// with no parametric keys stops importing as a no-op); the writer's
/// all-or-none condition reduced to the regions (a moved split alone never
/// reaches the sidecar); a key left out of `owned_attr_keys` (the merge
/// writes a second copy beside Lightroom's).
#[test]
fn the_parametric_curve_round_trips_as_lightroom_writes_it() {
    let doc = lr_parametric_doc();
    let r = xmp_to_recipe(&doc);
    assert_eq!(
        [r.param_shadows, r.param_darks, r.param_lights, r.param_highlights],
        [0.0, 40.0, -15.0, 0.0]
    );
    assert_eq!(r.parametric_splits(), [25.0, 50.0, 80.0]);

    let out = merge_recipe_into_xmp(&doc, &r).expect("mergeable");
    for spelling in [
        "crs:ParametricShadows=\"0\"",
        "crs:ParametricDarks=\"+40\"",
        "crs:ParametricLights=\"-15\"",
        "crs:ParametricHighlights=\"0\"",
        "crs:ParametricShadowSplit=\"25\"",
        "crs:ParametricMidtoneSplit=\"50\"",
        "crs:ParametricHighlightSplit=\"80\"",
    ] {
        assert!(out.doc.contains(spelling), "{spelling} missing: {}", out.doc);
        let key = &spelling[..spelling.find('=').expect("a key") + 1];
        assert_eq!(out.doc.matches(key).count(), 1, "{key} must appear exactly once");
    }
    let round = xmp_to_recipe(&out.doc);
    assert_eq!(
        (round.parametric_regions(), round.parametric_splits()),
        (r.parametric_regions(), r.parametric_splits()),
        "…and it reads back as itself"
    );

    // A moved split with every region at rest is still a statement.
    let split_only = EditRecipe { param_midtone_split: 60.0, ..Default::default() };
    let written = recipe_to_xmp(&split_only);
    assert!(written.contains("crs:ParametricMidtoneSplit=\"60\""), "{written}");
    assert!(written.contains("crs:ParametricDarks=\"0\""), "the block goes out whole: {written}");

    // A recipe that never moved the curve writes none of it, and a
    // document that names none of it imports as nothing.
    let neutral = recipe_to_xmp(&EditRecipe::default());
    assert!(!neutral.contains("Parametric"), "{neutral}");
    assert!(xmp_to_recipe(&neutral).is_noop());
}

/// v1.5.0, the era gate on the case it was generalised for: a v1.4
/// `recipe.json` (era 1) beside a Lightroom sidecar with a parametric
/// curve that recipe never saw. Its save must leave the curve standing —
/// and must still publish the R25 keys that recipe DOES own.
///
/// MUTATION THIS CATCHES: `WHOLE_BLOCKS` without `param_` (the gate
/// releases the block key by key and an edited region writes half of it),
/// or the gate's era range starting at era 1 (see the test above).
#[test]
fn a_v1_4_recipe_keeps_the_parametric_curve_it_never_had() {
    let doc = lr_parametric_doc();
    let mut legacy = as_v1_4_recipe(&xmp_to_recipe(&doc));
    assert_eq!(legacy.schema_era, 1);
    assert_eq!(legacy.param_darks, 0.0, "premise: serde filled the absent field");
    legacy.texture = 0.0; // an R25 key the v1.4 recipe owns, cleared here
    let out = merge_recipe_into_xmp(&doc, &legacy).expect("mergeable");
    assert!(out.doc.contains("crs:ParametricDarks=\"+40\""), "the curve was deleted: {}", out.doc);
    assert!(out.doc.contains("crs:ParametricHighlightSplit=\"80\""), "{}", out.doc);
    assert_eq!(out.doc.matches("crs:ParametricDarks=").count(), 1);
    assert!(out.doc.contains("crs:Texture=\"0\""), "an era-1 recipe still owns Texture: {}", out.doc);

    // One region moved on the legacy recipe releases the WHOLE block.
    legacy.param_lights = 30.0;
    let out = merge_recipe_into_xmp(&doc, &legacy).expect("mergeable");
    for key in ["ParametricShadows", "ParametricDarks", "ParametricLights", "ParametricHighlightSplit"] {
        assert_eq!(out.doc.matches(&format!("crs:{key}=")).count(), 1, "crs:{key}: {}", out.doc);
    }
    assert!(out.doc.contains("crs:ParametricLights=\"+30\""), "{}", out.doc);
    assert!(out.doc.contains("crs:ParametricDarks=\"0\""), "the recipe's own 0 publishes: {}", out.doc);
}

/// A Lightroom sidecar with the v1.5.0 colour blocks in the shape the
/// operator's library carries them: the B&W mixer where Lightroom writes it
/// (two bands moved), the Calibration seven (three moved) and the B&W switch
/// on — beside a Basic-panel exposure.
fn lr_colour_doc() -> String {
    "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Adobe XMP Core 5.6-c145\">\n\
         \x20<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         \x20 <rdf:Description rdf:about=\"\"\n\
         \x20   xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n\
         \x20   crs:Version=\"15.5.1\"\n\
         \x20   crs:ProcessVersion=\"15.4\"\n\
         \x20   crs:Exposure2012=\"+0.35\"\n\
         \x20   crs:GrayMixerRed=\"0\"\n\
         \x20   crs:GrayMixerOrange=\"0\"\n\
         \x20   crs:GrayMixerYellow=\"0\"\n\
         \x20   crs:GrayMixerGreen=\"0\"\n\
         \x20   crs:GrayMixerAqua=\"-1\"\n\
         \x20   crs:GrayMixerBlue=\"-44\"\n\
         \x20   crs:GrayMixerPurple=\"0\"\n\
         \x20   crs:GrayMixerMagenta=\"0\"\n\
         \x20   crs:ShadowTint=\"+3\"\n\
         \x20   crs:RedHue=\"0\"\n\
         \x20   crs:RedSaturation=\"0\"\n\
         \x20   crs:GreenHue=\"0\"\n\
         \x20   crs:GreenSaturation=\"0\"\n\
         \x20   crs:BlueHue=\"-28\"\n\
         \x20   crs:BlueSaturation=\"+83\"\n\
         \x20   crs:ConvertToGrayscale=\"True\"\n\
         \x20   crs:HasSettings=\"True\"/>\n\
         \x20</rdf:RDF>\n\
         </x:xmpmeta>\n"
        .to_string()
}

/// v1.5.0: Calibration and the B&W treatment read in as Lightroom states
/// them and write back as the whole blocks Lightroom writes, each key once.
///
/// MUTATIONS THIS CATCHES: a calibration or `GrayMixer*` key left out of
/// `owned_attr_keys` (the merge writes a second copy beside Lightroom's);
/// either block's condition reduced to its moved members; the B&W switch
/// written while off, or read off a creative Look.
#[test]
fn calibration_and_black_and_white_round_trip_as_lightroom_writes_them() {
    let doc = lr_colour_doc();
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.calibration(), Some([3.0, 0.0, 0.0, 0.0, 0.0, -28.0, 83.0]));
    assert!(r.convert_to_grayscale);
    assert_eq!(r.gray_mixer(), [0.0, 0.0, 0.0, 0.0, -1.0, -44.0, 0.0, 0.0]);
    assert!(unparsable_crs_numbers(&doc).is_empty(), "{:?}", unparsable_crs_numbers(&doc));
    assert!(unmodelled_global_crs(&doc).is_empty(), "{:?}", unmodelled_global_crs(&doc));

    let out = merge_recipe_into_xmp(&doc, &r).expect("mergeable");
    for spelling in [
        "crs:ShadowTint=\"+3\"",
        "crs:RedHue=\"0\"",
        "crs:BlueHue=\"-28\"",
        "crs:BlueSaturation=\"+83\"",
        "crs:GrayMixerRed=\"0\"",
        "crs:GrayMixerAqua=\"-1\"",
        "crs:GrayMixerBlue=\"-44\"",
        "crs:ConvertToGrayscale=\"True\"",
    ] {
        assert!(out.doc.contains(spelling), "{spelling} missing: {}", out.doc);
        let key = &spelling[..spelling.find('=').expect("a key") + 1];
        assert_eq!(out.doc.matches(key).count(), 1, "{key} must appear exactly once");
    }
    let round = xmp_to_recipe(&out.doc);
    assert_eq!(
        (round.calibration(), round.convert_to_grayscale, round.gray_mixer()),
        (r.calibration(), r.convert_to_grayscale, r.gray_mixer()),
        "…and it reads back as itself"
    );

    // One moved slider sends its whole block; B&W with an untouched mix
    // sends the eight at 0, the shape Lightroom writes.
    let one = recipe_to_xmp(&EditRecipe { cal_green_sat: 37.0, ..Default::default() });
    assert!(one.contains("crs:GreenSaturation=\"+37\"") && one.contains("crs:ShadowTint=\"0\""), "{one}");
    assert!(!one.contains("GrayMixer") && !one.contains("ConvertToGrayscale"), "{one}");
    let bw = recipe_to_xmp(&EditRecipe { convert_to_grayscale: true, ..Default::default() });
    assert!(bw.contains("crs:ConvertToGrayscale=\"True\""), "{bw}");
    assert!(bw.contains("crs:GrayMixerRed=\"0\"") && bw.contains("crs:GrayMixerMagenta=\"0\""), "{bw}");
    assert!(!bw.contains("crs:ShadowTint"), "{bw}");

    // A recipe that never moved them writes none of it, and a document
    // that names none of it imports as nothing.
    //
    // Each key SPELT WITH ITS PREFIX, never bare: `crs:DefringeGreenHueHi`
    // — which a neutral recipe does write, at Adobe's own default —
    // contains the bare `GreenHue`, so the bare form asserted that the
    // de-fringe block was absent and failed on a correct writer.
    let neutral = recipe_to_xmp(&EditRecipe::default());
    for key in CALIBRATION_CRS.iter().chain(&["ConvertToGrayscale", "GrayMixer"]) {
        let spelt = format!("crs:{key}");
        assert!(!neutral.contains(&spelt), "{spelt}: {neutral}");
    }
    assert!(xmp_to_recipe(&neutral).is_noop());

    // A switch Lightroom writes OFF is off…
    let off = doc.replace("crs:ConvertToGrayscale=\"True\"", "crs:ConvertToGrayscale=\"False\"");
    assert!(!xmp_to_recipe(&off).convert_to_grayscale);
    // …and a creative Look's own switch and calibration are the Look's.
    let look = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
                    xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
                    <rdf:Description rdf:about=\"\" \
                    xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\">\
                    <crs:BlueHue>-7</crs:BlueHue>\
                    <crs:Look><rdf:Description><crs:Parameters><rdf:Description>\
                    <crs:ConvertToGrayscale>True</crs:ConvertToGrayscale>\
                    <crs:BlueHue>+50</crs:BlueHue>\
                    </rdf:Description></crs:Parameters></rdf:Description></crs:Look>\
                    </rdf:Description></rdf:RDF></x:xmpmeta>";
    let r = xmp_to_recipe(look);
    assert!(!r.convert_to_grayscale, "the Look's B&W switch is the Look's");
    assert_eq!(r.cal_blue_hue, -7.0, "the Description's own calibration, never the Look's");
}

/// One Lightroom point-colour item: the sampled swatch MIDI2LR's
/// `LocalPresets.lua` dumps (SrcHue 1.312043 rad, every shift at -1), in
/// the nineteen-number order `PointColor::to_numbers` states.
const LR_SWATCH: &str = "1.312043, 0.473663, 0.739782, -1.000000, -1.000000, -1.000000, 0.500000, \
                             0.000000, 0.330000, 0.670000, 1.000000, 0.000000, 0.290000, 0.650000, 1.000000, \
                             0.150000, 0.700000, 1.000000, 1.000000";

/// The no-swatch placeholder Lightroom writes on 113 of the operator's 175
/// sidecars: one item of nineteen `-1.000000`s.
fn lr_placeholder_item() -> String {
    ["-1.000000"; 19].join(", ")
}

/// A Lightroom sidecar whose `crs:PointColors` holds `items`.
fn lr_point_colors_doc(items: &[&str]) -> String {
    let lis: String = items.iter().map(|i| format!("     <rdf:li>{i}</rdf:li>\n")).collect();
    format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Adobe XMP Core 5.6-c145\">\n\
             \x20<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
             \x20 <rdf:Description rdf:about=\"\"\n\
             \x20   xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n\
             \x20   crs:Version=\"15.5.1\"\n\
             \x20   crs:Exposure2012=\"+0.35\"\n\
             \x20   crs:HasSettings=\"True\">\n\
             \x20  <crs:PointColors>\n\
             \x20   <rdf:Seq>\n\
             {lis}\
             \x20   </rdf:Seq>\n\
             \x20  </crs:PointColors>\n\
             \x20 </rdf:Description>\n\
             \x20</rdf:RDF>\n\
             </x:xmpmeta>\n"
    )
}

/// v1.5.0: point colours read in from Lightroom's nineteen numbers, write
/// back where and as Lightroom writes them, and leave Lightroom's own
/// no-swatch placeholder alone.
///
/// MUTATIONS THIS CATCHES: the placeholder read as a swatch, or stripped by
/// an ordinary save (the merge owning the element unconditionally); a
/// legacy recipe's save deleting swatches it never saw (the era read
/// dropped); an item written in another order or precision; the element
/// left off `OWNED_ELEMENT_ONLY` (a second `crs:PointColors` beside
/// Lightroom's, and the import disclosure naming a property this engine
/// renders).
#[test]
fn point_colors_round_trip_and_leave_the_placeholder_alone() {
    // Lightroom's placeholder: nothing to import, nothing to disclose, and
    // an ordinary save keeps it where it stands.
    let placeholder = lr_point_colors_doc(&[lr_placeholder_item().as_str()]);
    let r = xmp_to_recipe(&placeholder);
    assert!(r.point_colors.is_empty(), "{:?}", r.point_colors);
    assert!(unparsable_crs_numbers(&placeholder).is_empty(), "{:?}", unparsable_crs_numbers(&placeholder));
    assert!(unmodelled_global_crs(&placeholder).is_empty(), "{:?}", unmodelled_global_crs(&placeholder));
    let kept = merge_recipe_into_xmp(&placeholder, &r).expect("mergeable");
    assert_eq!(kept.doc.matches("<crs:PointColors>").count(), 1, "{}", kept.doc);
    assert!(kept.doc.contains(&lr_placeholder_item()), "the placeholder stands: {}", kept.doc);

    // A real swatch reads in number for number…
    let doc = lr_point_colors_doc(&[LR_SWATCH]);
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.point_colors.len(), 1);
    let p = &r.point_colors[0];
    assert_eq!((p.src_hue, p.src_sat, p.src_lum), (1.312043, 0.473663, 0.739782));
    assert_eq!((p.hue_shift, p.sat_scale, p.lum_scale, p.range_amount), (-1.0, -1.0, -1.0, 0.5));
    assert_eq!((p.hue_range, p.sat_range, p.lum_range), (
        [0.0, 0.33, 0.67, 1.0],
        [0.0, 0.29, 0.65, 1.0],
        [0.15, 0.7, 1.0, 1.0]
    ));
    // …and merges back as the very item it arrived as, once.
    let out = merge_recipe_into_xmp(&doc, &r).expect("mergeable");
    assert_eq!(out.doc.matches("<crs:PointColors>").count(), 1, "{}", out.doc);
    assert!(out.doc.contains(&format!("<rdf:li>{LR_SWATCH}</rdf:li>")), "{}", out.doc);
    assert_eq!(xmp_to_recipe(&out.doc).point_colors, r.point_colors);
    // Where Lightroom writes it: after the curves, before the masks.
    let fresh = recipe_to_xmp(&EditRecipe {
        tone_curve: vec![
            crate::recipe::CurvePoint { input: 0, output: 0 },
            crate::recipe::CurvePoint { input: 128, output: 140 },
            crate::recipe::CurvePoint { input: 255, output: 255 },
        ],
        masks: vec![crate::recipe::LocalAdjustment {
            mask: crate::recipe::MaskGeometry::Linear { zero_x: 0.0, zero_y: 0.0, full_x: 0.0, full_y: 1.0 },
            enabled: true,
            amount: 1.0,
            exposure_ev: 0.5,
            ..Default::default()
        }],
        point_colors: r.point_colors.clone(),
        ..Default::default()
    });
    let at = |needle: &str| fresh.find(needle).unwrap_or_else(|| panic!("{needle} missing: {fresh}"));
    assert!(at("</crs:ToneCurvePV2012>") < at("<crs:PointColors>"), "{fresh}");
    assert!(at("</crs:PointColors>") < at("<crs:MaskGroupBasedCorrections>"), "{fresh}");

    // The develop DELETED its swatch: a current-era recipe speaks for the
    // element, and the base's goes.
    let cleared = EditRecipe { point_colors: Vec::new(), ..r.clone() };
    let out = merge_recipe_into_xmp(&doc, &cleared).expect("mergeable");
    assert!(!out.doc.contains("<crs:PointColors>"), "{}", out.doc);

    // A recipe from before v1.5.0 never saw the element: its save keeps it.
    let legacy = as_v1_4_recipe(&r);
    assert!(legacy.point_colors.is_empty() && legacy.schema_era == 1, "premise");
    let out = merge_recipe_into_xmp(&doc, &legacy).expect("mergeable");
    assert_eq!(out.doc.matches("<crs:PointColors>").count(), 1, "{}", out.doc);
    assert!(out.doc.contains(&format!("<rdf:li>{LR_SWATCH}</rdf:li>")), "{}", out.doc);

    // A recipe with a swatch replaces the placeholder: one element, ours.
    let out = merge_recipe_into_xmp(&placeholder, &r).expect("mergeable");
    assert_eq!(out.doc.matches("<crs:PointColors>").count(), 1, "{}", out.doc);
    assert!(!out.doc.contains(&lr_placeholder_item()), "{}", out.doc);
    assert!(out.notes.is_empty(), "{:?}", out.notes);
}

/// A `crs:PointColors` this reader refuses is NAMED, kept by a save that
/// has nothing to put in its place, and replaced out loud by one that has.
///
/// MUTATIONS THIS CATCHES: `from_numbers` repairing a record it does not
/// understand; the reader keeping the first sixteen of a longer list; the
/// merge owning an unreadable base block for a recipe with no swatch (a
/// list in some future Lightroom layout deleted on Ctrl+S); the
/// replacement note dropped.
#[test]
fn an_unreadable_point_color_block_is_named_kept_and_replaced_out_loud() {
    let eighteen = LR_SWATCH.rsplit_once(", ").expect("nineteen numbers").0;
    let twenty = format!("{LR_SWATCH}, 0.000000");
    let src_sat_above_one = LR_SWATCH.replacen("0.473663", "1.473663", 1);
    let backwards_window = LR_SWATCH.replacen("0.290000, 0.650000", "0.650000, 0.290000", 1);
    let seventeen = [LR_SWATCH; crate::recipe::MAX_POINT_COLORS + 1];
    for doc in [
        lr_point_colors_doc(&[eighteen]),
        lr_point_colors_doc(&[twenty.as_str()]),
        lr_point_colors_doc(&[src_sat_above_one.as_str()]),
        lr_point_colors_doc(&[backwards_window.as_str()]),
        lr_point_colors_doc(&[LR_SWATCH, "not, a, number"]),
        lr_point_colors_doc(&seventeen),
    ] {
        assert_eq!(unparsable_crs_numbers(&doc), vec!["PointColors"], "{doc}");
        let r = xmp_to_recipe(&doc);
        assert!(r.point_colors.is_empty(), "refused, never repaired: {doc}");
        // Nothing to put in its place: the base's own bytes stand.
        let body = owned_element_body(&doc, "crs:PointColors").expect("closed").expect("present");
        let kept = merge_recipe_into_xmp(&doc, &r).expect("mergeable");
        assert!(kept.doc.contains(body), "{}", kept.doc);
        assert!(kept.notes.is_empty(), "{:?}", kept.notes);
        // A develop with a swatch of its own replaces it — and says so.
        let mine = EditRecipe { point_colors: vec![crate::recipe::PointColor::sampled(1.0, 0.5, 0.5)], ..r };
        let replaced = merge_recipe_into_xmp(&doc, &mine).expect("mergeable");
        assert_eq!(replaced.doc.matches("<crs:PointColors>").count(), 1, "{}", replaced.doc);
        assert!(!replaced.doc.contains(body), "{}", replaced.doc);
        assert_eq!(replaced.notes.len(), 1, "{:?}", replaced.notes);
        assert!(replaced.notes[0].contains("point colours could not be read"), "{}", replaced.notes[0]);
    }
}

/// R25 P8, the READ / WRITE asymmetry: a document whose top-level child
/// opens and closes under DIFFERENT names balances its tag counts but
/// crosses its names. `top_level_owned_spans` (the writer's strip) does
/// not even notice — it tracks OWNED children only — so the merge went
/// ahead, while `crs_scope_inner` bailed, and a bailed scope hands every
/// scanner the WHOLE document: the creative Look's baked `crs:Clarity2012`
/// was then read as the photographer's own slider, which is the one thing
/// the scope function exists to prevent.
#[test]
fn a_crossed_name_look_is_dropped_from_the_scope_not_promoted_by_it() {
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Adobe XMP Core\">\n\
             \x20<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
             \x20 <rdf:Description rdf:about=\"\"\n\
             \x20   xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n\
             \x20   crs:Exposure2012=\"+0.35\"\n\
             \x20   crs:HasSettings=\"True\">\n\
             \x20  <crs:Look>\n\
             \x20   <rdf:Description>\n\
             \x20    <crs:Clarity2012>+50</crs:Clarity2012>\n\
             \x20   </rdf:Description>\n\
             \x20  </crs:Foo>\n\
             \x20 </rdf:Description>\n\
             \x20</rdf:RDF>\n\
             </x:xmpmeta>\n";
    let r = xmp_to_recipe(doc);
    assert_eq!(r.exposure_ev, 0.35, "the top level's own settings still import");
    assert_eq!(
        r.clarity, 0.0,
        "the Look's baked clarity is NOT this photographer's slider: {doc}"
    );
    // The writer really does go ahead on this document — which is what
    // made the reader's bail an asymmetry rather than a shared refusal.
    assert!(
        merge_recipe_into_xmp(doc, &EditRecipe::default()).is_some(),
        "premise: the merge does not refuse this shape"
    );
}

/// The other side of the same rule: the moment the user edits a mask, the
/// develop in hand IS the newest intent, so it publishes — and the base's
/// block going is a disclosed note, never a silence.
#[test]
fn an_edited_mask_overwrites_and_says_so() {
    let doc = lr_doc(&lr_correction("Radial 1", "", &lr_radial("37.412506", "0")));
    let mut r = xmp_to_recipe(&doc);
    r.masks[0].exposure_ev = 1.25;
    let out = merge_recipe_into_xmp(&doc, &r).expect("mergeable");
    assert!(
        !out.doc.contains("crs:Angle=\"37.412506\""),
        "the develop's own projection replaced the block: {}",
        out.doc
    );
    assert_eq!(out.notes.len(), 1, "the replacement is disclosed: {:?}", out.notes);
    assert!(
        out.notes[0].contains("1 thing(s)") && out.notes[0].contains("1 edited mask(s)"),
        "the note names both counts: {}",
        out.notes[0]
    );
    assert_eq!(
        xmp_to_recipe(&out.doc).masks[0].exposure_ev,
        1.25,
        "and the edit is what the file now says"
    );
}

/// R35 preserves every ordered component. The base selector still owes
/// the R25 P9 rule: composition must start at the BASE — the
/// first component in the file, which is what Lightroom's Add/Subtract
/// stack composes onto. It used to be decided by KIND instead: the reader
/// tried `Mask/Gradient` before `Mask/CircularGradient`, so a trailing
/// linear beat a leading radial and the imported mask was a shape the
/// correction merely happened to also contain.
///
/// Measured on the user's library, not invented: `P29` 蒙版 5 is
/// `[CircularGradient, CircularGradient, Gradient]` and imported as that
/// trailing LINEAR with both radials gone; `P12` Mask 9 is
/// `[CircularGradient, Gradient]` and did the same. The loss was DISCLOSED
/// throughout (`MultiComponent`, and the dropped radials' own
/// `Rotation(…)` notes) — so this was never the silent drop it looked
/// like from the recipe alone — but the surviving shape was the wrong one.
///
/// MUTATION THIS CATCHES: restoring the kind-ordered `if let` chain flips
/// row 1 back to a linear; ignoring `crs:MaskBlendMode` in the selector
/// flips rows 3 and 4, the two that are inversions of intent rather than
/// truncations of it.
#[test]
fn a_multi_component_correction_imports_its_base_and_every_ordered_shape() {
    // A subtract component, spelled the way Lightroom spells it (the pair
    // v0.31.1 taught this reader to read: mode "1" WITH MaskValue "0").
    let subtract = |c: String| {
        c.replace("crs:MaskValue=\"1\"", "crs:MaskValue=\"0\"")
    };
    let radial = || lr_radial("0", "0");
    for (label, comps, want_radial) in [
        // Kind order used to decide: a TRAILING linear beat a leading
        // radial. `P29` 蒙版 5's real structure, all three at the
        // default blend mode — a plain union.
        ("radial, radial, linear (union)", vec![radial(), radial(), lr_gradient("0")], true),
        ("linear, radial (union)", vec![lr_gradient("0"), radial()], false),
        // Blend mode decides over document order: `P29` 蒙版 3 and
        // `P12` Mask 9 are both a base radial + a SUBTRACT linear, and
        // the importer kept the shape Lightroom carves away WITH.
        ("radial base, linear subtract", vec![radial(), subtract(lr_gradient("1"))], true),
        ("linear subtract, radial base", vec![subtract(lr_gradient("1")), radial()], true),
        // Nothing but subtractions has no base to find, so the first
        // component stands in — some shape beats no shape.
        (
            "all subtract",
            vec![subtract(lr_gradient("1")), subtract(lr_radial("0", "1"))],
            false,
        ),
    ] {
        let doc = lr_doc(&lr_correction("Stacked 1", "", &comps.concat()));
        let r = xmp_to_recipe(&doc);
        assert_eq!(r.masks.len(), 1, "{label}: one correction imports one composed mask");
        let got_radial = matches!(r.masks[0].mask, MaskGeometry::Radial { .. });
        assert_eq!(
            got_radial, want_radial,
            "{label}: wrong base — got {:?}",
            r.masks[0].mask
        );
        assert_eq!(r.masks[0].components.len(), comps.len() - 1, "{label}");
        let losses = import_losses(&doc);
        assert!(!losses.iter().any(|l| l.reason == MaskImportReason::MultiComponent), "{label}: {losses:?}");
    }
}

/// Every radial now ARRIVES, so every withheld radial rotation must be
/// disclosed. Native component blend modes do not lose their composition.
/// Restoring the old base-only filter would hide the imported component's
/// withheld angle; restoring the old blend warning would invent a loss.
#[test]
fn every_imported_radials_withheld_rotation_is_disclosed_and_composition_is_preserved() {
    // Base = an UNROTATED linear; the imported component carries the angle.
    let comps = format!("{}{}", lr_gradient("0"), lr_radial("37.412506", "0"));
    let doc = lr_doc(&lr_correction("Stacked 1", "", &comps));
    let reasons: Vec<_> = import_losses(&doc).into_iter().map(|l| l.reason).collect();
    assert!(
        matches!(r_kind(&doc), MaskKindForTest::Linear),
        "the unrotated linear is the base here"
    );
    assert!(
        reasons.contains(&MaskImportReason::Rotation(37)),
        "the imported component's withheld rotation must be named: {reasons:?}"
    );
    assert!(
        !reasons.contains(&MaskImportReason::MultiComponent),
        "no shape is dropped: {reasons:?}"
    );
    // The mirror: when the ROTATED radial is the base, the note is true and
    // must still fire.
    let comps = format!("{}{}", lr_radial("37.412506", "0"), lr_gradient("0"));
    let doc = lr_doc(&lr_correction("Stacked 1", "", &comps));
    let reasons: Vec<_> = import_losses(&doc).into_iter().map(|l| l.reason).collect();
    assert!(
        reasons.contains(&MaskImportReason::Rotation(37)),
        "the imported radial's own rotation is a real loss: {reasons:?}"
    );
    // And a dropped SUBTRACT component keeps its own note (v0.31.1).
    let comps = format!(
        "{}{}",
        lr_radial("0", "0"),
        lr_gradient("1").replace("crs:MaskValue=\"1\"", "crs:MaskValue=\"0\"")
    );
    let doc = lr_doc(&lr_correction("Stacked 1", "", &comps));
    let reasons: Vec<_> = import_losses(&doc).into_iter().map(|l| l.reason).collect();
    assert!(
        !reasons.contains(&MaskImportReason::BlendMode),
        "the subtract component is now composed: {reasons:?}"
    );
}

enum MaskKindForTest {
    Linear,
    Other,
}

fn r_kind(doc: &str) -> MaskKindForTest {
    match xmp_to_recipe(doc).masks.first().map(|m| &m.mask) {
        Some(MaskGeometry::Linear { .. }) => MaskKindForTest::Linear,
        _ => MaskKindForTest::Other,
    }
}

/// R25 P1, the import twin of `mask_loss_reason_all_covers_every_variant`:
/// both disclosure surfaces ITERATE `MaskImportReason::ALL` (here and the
/// GUI's `xmp_import_line`), so the list is the one place a reason can be
/// forgotten — and the match below is where a new variant stops the build.
#[test]
fn import_loss_reasons_all_reach_the_prose() {
    // Adding a variant makes THIS match non-exhaustive; the arm you write
    // carries the next rank, and the asserts fail until `ALL` lists the
    // newcomer in that position.
    fn rank(r: MaskImportReason) -> usize {
        match r {
            MaskImportReason::Unrepresentable => 0,
            MaskImportReason::OutOfModel => 1,
            MaskImportReason::Rotation(_) => 2,
            MaskImportReason::BlendMode => 3,
            MaskImportReason::MultiComponent => 4,
            MaskImportReason::BrushRendered => 5,
            MaskImportReason::AiMaskRecomputed => 6,
            MaskImportReason::AiMaskUnresolved => 7,
            MaskImportReason::ForeignRangeMask => 8,
            MaskImportReason::LocalCurve => 9,
            MaskImportReason::CurveRefineSaturation => 10,
            MaskImportReason::InertLocal(_) => 11,
            MaskImportReason::UnknownLocalKey => 12,
        }
    }
    for (i, r) in MaskImportReason::ALL.into_iter().enumerate() {
        assert_eq!(rank(r), i, "ALL must list every reason once, in rank order");
        assert!(!r.en().trim().is_empty(), "{r:?} has no label for the prose channel");
        assert!(r.same_kind(r), "same_kind must be reflexive for {r:?}");
    }
    // The payload variantS group by KIND, not by value — the property the
    // prose channels rely on to print one line for two sliders, and (since
    // R25 P5) one line for two differently-tilted radials.
    assert!(
        MaskImportReason::InertLocal("LocalGrain")
            .same_kind(MaskImportReason::InertLocal("LocalMoire")),
        "two unmodelled sliders are one line"
    );
    assert!(
        MaskImportReason::Rotation(37).same_kind(MaskImportReason::Rotation(-44)),
        "two tilted radials are one line"
    );
    assert!(
        !MaskImportReason::Rotation(0).same_kind(MaskImportReason::BlendMode),
        "different variants are different lines"
    );
    // Exactly the two drop verdicts, and they are the ones
    // `unsupported_corrections` counts.
    assert_eq!(
        MaskImportReason::ALL.into_iter().filter(|r| r.is_drop()).count(),
        2,
        "a third drop reason needs `unsupported_corrections`' doc revisited"
    );
    let losses: Vec<MaskImportLoss> = MaskImportReason::ALL
        .into_iter()
        .map(|reason| MaskImportLoss { name: format!("m{}", rank(reason)), reason })
        .collect();
    let line = describe_import_losses(3, &losses).expect("ten losses ⇒ a line");
    assert!(line.contains("imported 3 Lightroom mask(s)"), "{line}");
    for r in MaskImportReason::ALL {
        assert!(line.contains(r.en()), "{r:?} never reaches the prose: {line}");
        assert!(
            line.contains(&format!("m{}", rank(r))),
            "{r:?} loses its correction name: {line}"
        );
    }
}

// ── R25 P6: the four LOCAL point curves ──────────────────────────────

/// The round trip, in both directions, over all four keys and their
/// SPARSENESS. The fixture reproduces `P51.xmp`'s own shape: Red and
/// Green present, Main and Blue absent.
#[test]
fn local_curves_round_trip() {
    let curves = format!(
        "{}{}",
        lr_curve("RedCurve", &[(0, 0), (239, 255)]),
        lr_curve("GreenCurve", &[(0, 12), (128, 140), (255, 255)]),
    );
    let doc = lr_doc(&lr_correction_with_curves(
        "Radial 1",
        "",
        &curves,
        &lr_radial("0", "0"),
    ));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "the correction must import: {:?}", r.masks);
    let m = &r.masks[0];
    assert_eq!(
        m.red_curve,
        vec![
            CurvePoint { input: 0, output: 0 },
            CurvePoint { input: 239, output: 255 },
        ],
        "crs:RedCurve did not reach the recipe"
    );
    assert_eq!(m.green_curve.len(), 3, "crs:GreenCurve: {:?}", m.green_curve);
    assert!(m.main_curve.is_empty() && m.blue_curve.is_empty(), "absent means absent");

    // …and back out through OUR writer, then in again: the curves survive
    // byte-for-byte as VALUES (the writer's own spelling is pinned by
    // `local_curve_serialization_has_no_space_after_the_comma`).
    let back = xmp_to_recipe(&recipe_to_xmp(&r));
    assert_eq!(back.masks.len(), 1, "the mask survives our own projection");
    assert_eq!(back.masks[0].red_curve, m.red_curve, "red curve lost in the round trip");
    assert_eq!(back.masks[0].green_curve, m.green_curve, "green curve lost");
    assert!(
        back.masks[0].main_curve.is_empty() && back.masks[0].blue_curve.is_empty(),
        "the writer invented a curve the recipe does not hold"
    );
}

/// THE FORMAT MUTATION GUARD. Lightroom spells a LOCAL curve point
/// `x,y` and a GLOBAL one `x, y`. Nothing in the code enforces that but
/// two separate formatters and this test — and "let's share one helper" /
/// "let's make the spacing consistent" is exactly the tidy-up a later
/// reader would make.
///
/// MUTATION THIS CATCHES: adding a space in `local_curve_elem`, or
/// removing one from `owned_children`'s `curve_elem`. Both halves are
/// asserted in ONE document, so neither can be satisfied by accident.
#[test]
fn local_curve_serialization_has_no_space_after_the_comma() {
    let r = EditRecipe {
        // The global master curve, whose writer uses the SPACED form.
        tone_curve: vec![
            CurvePoint { input: 10, output: 20 },
            CurvePoint { input: 255, output: 255 },
        ],
        masks: vec![LocalAdjustment {
            name: "curved".into(),
            amount: 1.0,
            main_curve: vec![
                CurvePoint { input: 32, output: 48 },
                CurvePoint { input: 255, output: 255 },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    let xmp = recipe_to_xmp(&r);
    assert!(
        xmp.contains("<rdf:li>32,48</rdf:li>"),
        "a LOCAL curve point is spelled `x,y` with no space: {xmp}"
    );
    assert!(
        !xmp.contains("<rdf:li>32, 48</rdf:li>"),
        "the local writer grew the global writer's space: {xmp}"
    );
    assert!(
        xmp.contains("<rdf:li>10, 20</rdf:li>"),
        "the GLOBAL curve keeps its spaced form: {xmp}"
    );
    assert!(
        !xmp.contains("<rdf:li>10,20</rdf:li>"),
        "the global writer lost its space to the local one: {xmp}"
    );
    // The key is the BARE name and the element sits between the attribute
    // block and the mask list — the two other things the reference
    // sidecars pin and a shared helper would get wrong.
    assert!(xmp.contains("<crs:MainCurve>"), "the local key carries no PV2012 suffix: {xmp}");
    let after_refine = xmp
        .split_once("crs:LocalCurveRefineSaturation=\"100\">")
        .expect("the correction's attribute block closes there")
        .1;
    let curve_at = after_refine.find("<crs:MainCurve>").expect("the curve is emitted");
    let masks_at = after_refine.find("<crs:CorrectionMasks>").expect("the mask list is too");
    assert!(curve_at < masks_at, "the curve must precede <crs:CorrectionMasks>: {xmp}");
}

/// Sparse in, sparse OUT. Lightroom writes only the curves that exist, and
/// a writer that emitted all four (as identities, say) would hand the
/// photographer's sidecar three curves they never drew.
#[test]
fn sparse_curves_stay_sparse() {
    let r = EditRecipe {
        masks: vec![LocalAdjustment {
            name: "red only".into(),
            amount: 1.0,
            red_curve: vec![
                CurvePoint { input: 0, output: 0 },
                CurvePoint { input: 128, output: 160 },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    let xmp = recipe_to_xmp(&r);
    assert!(xmp.contains("<crs:RedCurve>"), "the curve that exists is written: {xmp}");
    for absent in ["<crs:MainCurve>", "<crs:GreenCurve>", "<crs:BlueCurve>"] {
        assert!(!xmp.contains(absent), "{absent} was invented out of an empty curve: {xmp}");
    }
    // A mask with NO curve at all writes none of the four — the common
    // case, and the one that keeps every pre-P6 sidecar byte-identical.
    let plain = recipe_to_xmp(&EditRecipe {
        masks: vec![LocalAdjustment { name: "plain".into(), amount: 1.0, ..Default::default() }],
        ..Default::default()
    });
    for absent in ["<crs:MainCurve>", "<crs:RedCurve>", "<crs:GreenCurve>", "<crs:BlueCurve>"] {
        assert!(!plain.contains(absent), "{absent} on a curveless mask: {plain}");
    }
}

/// R25 P1 raised `LocalCurve` on every correction that carried one of the
/// four curve elements, because the engine modelled none of them. P6
/// models all four — so a correction whose curves READ must now report
/// NOTHING, or the disclosure cries wolf on 19 of the user's photos.
#[test]
fn a_correction_with_curves_no_longer_reports_a_local_curve_loss() {
    let curves = lr_curve("MainCurve", &[(0, 0), (128, 96), (255, 255)]);
    let doc =
        lr_doc(&lr_correction_with_curves("Radial 1", "", &curves, &lr_radial("0", "0")));
    assert!(
        import_losses(&doc).is_empty(),
        "a correction whose curve imported must report no loss: {:?}",
        import_losses(&doc)
    );
    assert_eq!(unsupported_corrections(&doc), 0);
    assert_eq!(
        xmp_to_recipe(&doc).masks[0].main_curve.len(),
        3,
        "premise: the curve really did import (else the silence is a lie)"
    );
}

/// The other half of the narrowing: a curve that is PRESENT and cannot be
/// read is still a loss, and it is still NAMED. `parse_one_correction`
/// reads the four keys through the unchecked `parse_curve`, whose `Err`
/// half becomes an empty curve — this is what stops that from being
/// silence, which is the module's standing rule (`owned_element_body`).
///
/// Costing the CURVE and not the correction is deliberate: the geometry is
/// still exactly what the file draws, the same verdict a foreign range
/// mask gets.
#[test]
fn an_unreadable_local_curve_is_named_not_swallowed() {
    // "999,-5" is out of the 0..255 domain — the same input
    // `parse_curve_checked` refuses on the global curves (L05).
    let curves = lr_curve("BlueCurve", &[(0, 0), (255, 255)])
        .replace("<rdf:li>255,255</rdf:li>", "<rdf:li>999,-5</rdf:li>");
    let doc =
        lr_doc(&lr_correction_with_curves("Radial 1", "", &curves, &lr_radial("0", "0")));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "the mask still imports — the shape is readable");
    assert!(r.masks[0].blue_curve.is_empty(), "an unreadable curve imports as none");
    assert_eq!(
        import_losses(&doc),
        vec![MaskImportLoss {
            name: "Radial 1".into(),
            reason: MaskImportReason::LocalCurve
        }],
        "the loss must be named, not swallowed"
    );
}

// ── R27 Batch-4 (L-08): the brush arm ────────────────────────────────────

/// One `Mask/Paint` stroke. Attribute VALUES are `P12` Mask 7 →
/// Aggregate "Brush 1" verbatim (the F2 anatomy's reference specimen,
/// `P12.xmp` in the user's library, 75,935 B);
/// the indentation is not, because whitespace between attributes is
/// insignificant and a fixture whose mutations depend on counting spaces
/// is a fixture that tests the spaces.
fn lr_paint(sync: &str, value: &str, blend: &str, inverted: &str, dabs: &[&str]) -> String {
    let items: String =
        dabs.iter().map(|t| format!("<rdf:li>{t}</rdf:li>\n")).collect();
    format!(
        "<rdf:li>\n\
             <rdf:Description crs:What=\"Mask/Paint\" crs:MaskActive=\"true\"\n\
             crs:MaskBlendMode=\"{blend}\" crs:MaskInverted=\"{inverted}\"\n\
             crs:MaskSyncID=\"{sync}\" crs:MaskValue=\"{value}\"\n\
             crs:Radius=\"0.582157\" crs:Flow=\"1\" crs:CenterWeight=\"0\">\n\
             <crs:Dabs>\n\
             <rdf:Seq>\n\
             {items}\
             </rdf:Seq>\n\
             </crs:Dabs>\n\
             </rdf:Description>\n\
             </rdf:li>\n"
    )
}

/// Stroke 1 of `P12` Mask 7 → Brush 1: `MaskValue="0.439815"`,
/// `Radius="0.582157"`, and the eight dab tokens §1.1 of the anatomy
/// prints as its worked example.
fn lr_paint_specimen() -> String {
    lr_paint(
        "FA7459A9F5626F4881D7B730C3093F95",
        "0.439815",
        "0",
        "false",
        &[
            "r 0.581835",
            "d 0.000684 0.940004",
            "r 0.581172",
            "d 0.113862 0.987261",
            "r 0.580873",
            "d 0.229292 1.011389",
            "r 0.581205",
            "d 0.112441 1.007149",
        ],
    )
}

/// The `Mask/Aggregate` group itself — two strokes, the second exercising
/// the `f` and `h` state tokens. `(MaskBlendMode, MaskValue) = (0, 1)` is
/// Lightroom's plain ADD, 16 of the 39 real Aggregates; `extra_child` is
/// spliced as a THIRD member of `crs:Masks` for the nesting tests.
fn lr_brush_group(group_inverted: &str, extra_child: &str) -> String {
    let s1 = lr_paint_specimen();
    let s2 = lr_paint(
        "1111111111111111111111111111111A",
        "1",
        "0",
        "false",
        &["f 1", "h 1", "d 0.500000 0.500000"],
    );
    format!(
        "<rdf:li>\n\
             <rdf:Description crs:What=\"Mask/Aggregate\" crs:MaskActive=\"true\"\n\
             crs:MaskName=\"Brush 1\" crs:MaskBlendMode=\"0\"\n\
             crs:MaskInverted=\"{group_inverted}\"\n\
             crs:MaskSyncID=\"0000000000000000000000000000000D\" crs:MaskValue=\"1\">\n\
             <crs:Masks>\n\
             <rdf:Seq>\n\
             {extra_child}{s1}{s2}\
             </rdf:Seq>\n\
             </crs:Masks>\n\
             </rdf:Description>\n\
             </rdf:li>\n"
    )
}

/// The `P12` Mask 7 shape — a linear gradient plus the brush group —
/// imports WHOLE, where before R27 Batch-4 the whole correction was thrown
/// away and the gradient with it. That is the L-08 registration's own
/// complaint: 14 already-drawable parametric shapes across the reference
/// library were being discarded because a NEIGHBOURING component was a
/// brush.
///
/// MUTATION-LINED. Verified red three independent ways (transcripts in the
/// batch report): reverting the `"Mask/Aggregate"` arm of
/// `classify_correction` to `unknown_component = true`; deleting
/// `parse_one_correction`'s brush-component collection; never pushing
/// `MaskImportReason::BrushRendered`, which imports the correction SILENTLY
/// — the failure mode this project treats as worse than the refusal it
/// replaced.
/// R29 Batch-3: `crs:LensProfileEnable` is READ, in both spellings, and a
/// document that says nothing gets no opinion put in its mouth.
///
/// This is the fact that separates "no warp because Lightroom drew no
/// correction" (the frames coincide; identity is CORRECT) from "no warp
/// because nobody could solve one" (the frames differ by an unknown
/// amount). `MaskWarpSource` keeps them apart and this reader is what
/// supplies the first one.
#[test]
fn the_sidecar_lens_profile_switch_is_read_in_both_spellings() {
    let with = |v: &str| {
        lr_doc("").replace("crs:Version=", &format!("crs:LensProfileEnable=\"{v}\"\n    crs:Version="))
    };
    // PREMISE: the substitution really landed, or every case below is
    // reading a document with no such key and agreeing by accident.
    assert!(with("0").contains("crs:LensProfileEnable=\"0\""), "{}", with("0"));
    assert_eq!(lens_profile_enabled(&with("0")), Some(false));
    assert_eq!(lens_profile_enabled(&with("1")), Some(true));
    assert_eq!(lens_profile_enabled(&with("False")), Some(false));
    assert_eq!(lens_profile_enabled(&with("true")), Some(true));
    // Says nothing / says something unreadable ⇒ no opinion. Guessing here
    // would decide a coordinate frame from a value nobody wrote.
    assert_eq!(lens_profile_enabled(&lr_doc("")), None);
    assert_eq!(lens_profile_enabled(&with("maybe")), None);
    // `crs:LensProfileName` must not answer for `crs:LensProfileEnable` —
    // MUTATION THIS KILLS: dropping the `crs:` key anchoring in `crs_str`.
    let named = lr_doc("").replace(
        "crs:Version=",
        "crs:LensProfileName=\"Adobe (Sony FE 24-105mm F4 G OSS)\"\n    crs:Version=",
    );
    assert_eq!(lens_profile_enabled(&named), None);
}

/// R29 Batch-3 ACCEPTANCE ④ — the mask warp does NOT touch this boundary.
///
/// `LensProfile::mask_warp` is a RENDER-TIME map. The recipe keeps the
/// coordinates the sidecar stored, verbatim, so `lr_to_engine` and
/// `engine_to_lr` stay exact inverses of one another and a republished
/// sidecar is byte-faithful to what Lightroom wrote — brush dab streams
/// included, which is the payload with the least tolerance for a rewrite.
/// The frame here is LANDSCAPE, which is what keeps that claim whole after
/// R29 C1: a turn does rewrite the dab stream now, and the only thing that
/// must never reach this boundary is the WARP.
///
/// The document here carries a radial, a gradient and a two-stroke brush
/// group, and is exported twice: once from a recipe with no warp and once
/// from the same recipe carrying the full 105 mm warp — the most violent
/// one measured (`m` from 1.0425 at the centre to 0.9976 at the corner,
/// ~88 px at r = 3250). The two documents must be EQUAL, byte for byte.
///
/// MUTATION THIS KILLS: applying `render::lr_mask_warp_norm` inside
/// `masks_xml` / `radial_mask_xml` / `brush_mask_xml`, or anywhere else on
/// the way out. Any of them makes these two strings differ.
#[test]
fn the_mask_warp_never_reaches_the_xmp_boundary() {
    let frame = FrameAspect::from_size(9504.0, 6336.0);
    let doc = in_frame(
        &lr_doc(&format!(
            "{}{}",
            lr_correction("Mask 7", "", &format!("{}{}", lr_gradient("0"), lr_brush_group("false", ""))),
            lr_correction(
                "R",
                "",
                &lr_radial_at("-0.082402", "-0.008723", "1.109604", "1.090228", "28.229232")
            ),
        )),
        9504,
        6336,
    );
    let plain = xmp_to_recipe(&doc);
    // PREMISE: the document really did bring in the geometry whose frame
    // this test is about, or it would prove nothing.
    assert_eq!(plain.masks.len(), 2, "both corrections must import: {:?}", plain.masks);
    assert!(
        plain.masks.iter().any(|m| m
            .components
            .iter()
            .any(|c| matches!(c.geometry, MaskGeometry::Brush { .. }))),
        "the brush group must be in the recipe"
    );
    let mut warped = plain.clone();
    let model = crate::lcp::PerspectiveModel {
        focal_mm: Some(105.0),
        focus_distance: Some(10000.0),
        scale: 0.959207,
        k: [0.961677, 1.182717, -8.218554],
        focal_x: None,
        sensor_format_factor: 1.0,
        vignette: None,
    };
    let legacy_warp = model
        .mask_warp_knots((9504.0, 6336.0), 16)
        .expect("the legacy 105mm table solves");
    let dense_warp = model
        .mask_warp_knots((9504.0, 6336.0), crate::recipe::MASK_WARP_KNOTS)
        .expect("the dense 105mm table solves");
    let half_diag = 0.5f32 * 9504.0f32.hypot(6336.0);
    let radius = 3250.0f32;
    let rho = radius / half_diag;
    let tabulation_delta =
        (crate::render::mask_warp_factor(&dense_warp, rho)
            - crate::render::mask_warp_factor(&legacy_warp, rho))
        .abs()
            * radius;
    eprintln!("105mm mask-warp n=16→64 delta at r=3250: {tabulation_delta:.4}px");
    assert!(tabulation_delta < 0.35, "105mm survival bound: {tabulation_delta}px");
    warped.lens_profile = crate::recipe::LensProfile {
        mask_warp: dense_warp,
        mask_warp_src: crate::recipe::MaskWarpSource::Lcp,
        ..Default::default()
    };
    // PREMISE: the warp really is a warp — an identity table would make
    // the equality below vacuous.
    let w = &warped.lens_profile.mask_warp;
    assert_eq!(w.len(), crate::recipe::MASK_WARP_KNOTS);
    assert!(w[0] > 1.04 && w[w.len() - 1] < 1.0, "the 105mm warp is not the identity: {w:?}");

    // The PROJECTION (payload-free, v1.3.1): the payload carries the whole
    // recipe, lens profile included, so two whole documents differ by
    // design; what must not move is what Lightroom reads.
    let a = bare_document(&plain, frame);
    let b = bare_document(&warped, frame);
    assert_eq!(a, b, "an active mask warp changed the written sidecar");
    // And the ROUND TRIP still lands on the same recipe geometry, so the
    // equality above is not two identically-broken documents.
    //
    // Compared field by field rather than with one `assert_eq!` on the
    // masks, because `crs:MaskSyncID` legitimately differs: the writer
    // MINTS a fresh identity for every component it emits (see
    // `BrushStroke::sync_id`), so a whole-struct comparison would fail on
    // the one field that is supposed to change and say nothing about the
    // frame.
    let back = xmp_to_recipe(&b);
    assert_eq!(back.masks.len(), plain.masks.len());
    for (got, want) in back.masks.iter().zip(&plain.masks) {
        assert_eq!(got.mask, want.mask, "base geometry moved");
        assert_eq!(got.components.len(), want.components.len());
        for (g, w) in got.components.iter().zip(&want.components) {
            match (&g.geometry, &w.geometry) {
                (
                    MaskGeometry::Brush { strokes: gs, name: gn, .. },
                    MaskGeometry::Brush { strokes: ws, name: wn, .. },
                ) => {
                    assert_eq!(gn, wn);
                    assert_eq!(gs.len(), ws.len());
                    for (a, b) in gs.iter().zip(ws) {
                        // The DAB STREAM, token for token — the payload a
                        // coordinate warp would have rewritten.
                        assert_eq!(a.dabs, b.dabs, "a dab stream was rewritten");
                        assert_eq!((a.value, a.radius, a.flow, a.center_weight),
                            (b.value, b.radius, b.flow, b.center_weight));
                    }
                }
                (g, w) => assert_eq!(g, w, "component geometry moved"),
            }
        }
    }
}

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

#[test]
fn globals_round_trip_through_xmp() {
    // Values are chosen to survive the writer's documented rounding: integer
    // sliders (`signed()`), 2-decimal exposure, integer Kelvin, 1-decimal
    // straighten, %.6f crop — so the reader must land EXACTLY back.
    let r = EditRecipe {
        exposure_ev: 0.32,
        contrast: 14.0,
        highlights: -12.0,
        shadows: 25.0,
        whites: 8.0,
        blacks: -6.0,
        temperature_k: Some(5600.0),
        tint: 3.0,
        vibrance: 18.0,
        saturation: -5.0,
        clarity: 10.0,
        dehaze: 7.0,
        hsl: crate::recipe::Hsl {
            hue: [0.0, 15.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            saturation: [0.0, 0.0, 0.0, -40.0, 0.0, 0.0, 0.0, 0.0],
            ..Default::default()
        },
        color_grade: crate::recipe::ColorGrade {
            shadow_hue: 220.0,
            shadow_sat: 30.0,
            highlight_hue: 45.0,
            highlight_sat: 20.0,
            midtone_lum: -10.0,
            balance: 15.0,
            ..Default::default()
        },
        // → crs 45 → back as 45. Exact for a different reason than before
        // v0.31.1: not "45 happens to survive ×⅔ then ×1.5", but "there is
        // no scale in either direction any more".
        sharpening: 45.0,
        noise_reduction: 20.0,
        lens_vignette: 35.0,
        lens_vignette_mid: 60.0,
        lens_distortion: -24.0,
        straighten_deg: 1.5,
        crop: Some(Crop { left: 0.05, top: 0.0, right: 0.95, bottom: 1.0 }),
        tone_curve: vec![
            CurvePoint { input: 0, output: 8 },
            CurvePoint { input: 255, output: 247 },
        ],
        red_curve: vec![
            CurvePoint { input: 0, output: 10 },
            CurvePoint { input: 255, output: 250 },
        ],
        rationale: "warm & contrasty <test> & \"q\"".into(),
        confidence: 0.82,
        ..Default::default()
    };
    let back = xmp_to_recipe(&recipe_to_xmp(&r));
    assert_eq!(back, r);
}

#[test]
fn as_shot_tint_round_trips_only_for_our_own_sidecars() {
    // Our writer emits a non-neutral Tint even under "As Shot"; the AutoShade
    // marker tells the reader it is a real edit.
    let r = EditRecipe { tint: 3.0, ..Default::default() };
    assert_eq!(xmp_to_recipe(&recipe_to_xmp(&r)).tint, 3.0);
}

#[test]
fn parametric_masks_round_trip_geometry_and_original_inversion_homes() {
    let r = EditRecipe {
        masks: vec![
            LocalAdjustment {
                mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.35, full_x: 0.5, full_y: 0.0 },
                range: Some(RangeMask::Luminance { lo_outer: 0.4, lo: 0.5, hi: 1.0, hi_outer: 1.0 }),
                name: "sky & sea".into(),
                amount: 0.75,
                inverted: true,
                exposure_ev: -0.4, // ÷4 → ×4 is a power-of-two rescale: exact
                contrast: 30.0,    // "0.3" ×100 needs the 4-decimal snap: exact
                highlights: -50.0,
                shadows: 60.0,
                whites: 10.0,
                blacks: -20.0,
                clarity: 40.0,
                dehaze: 5.0,
                texture: 15.0,
                // R23-1b: two keys the writer used to emit as a literal
                // "0". They ride the same ÷100 ↔ ×100 pair as their
                // neighbours, and a sidecar carrying them must still import
                // as loss-free — `correction_value_reasons` demanded 0
                // for both until R23-1b, so a non-zero one would have
                // refused the whole correction and this equality would
                // fail on every other field too.
                sharpness: -45.0,
                saturation: 20.0,
                hue: 35.0,
                temperature: 25.0,
                tint: -10.0,
                noise_reduction: 30.0,
                ..Default::default()
            },
            LocalAdjustment {
                mask: MaskGeometry::Radial {
                    top: 0.3, left: 0.35, bottom: 0.7, right: 0.65,
                    feather: 0.5, roundness: 0.0, flipped: true, angle: 0.0,
                    midpoint: 50.0, mask_version: 2,
                },
                range: Some(RangeMask::Color { r: 0.9, g: 0.6, b: 0.2, amount: 0.5, px: 0.4, py: 0.7 }),
                name: "subject".into(),
                shadows: 20.0,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let back = xmp_to_recipe(&recipe_to_xmp(&r));
    // R25 P9: ONE field does not come back where it went in. Lightroom
    // spells a radial's inversion ONCE (`crs:MaskInverted`, with
    // `crs:Flipped` as its complement) where this recipe spells it as the
    // XOR of two flags, so the projection collapses `flipped` into
    // `inverted`. The XOR is the whole of what the pixels see (render.rs
    // `mask_weight` and the weight loop) and it survives exactly; WHICH of
    // our two flags carries it is not a fact about the photograph.
    // Written as an expected value rather than a relaxed comparison so
    // every other field still has to match to the bit.
    let mut expect = r.masks.clone();
    // …and ONE more, from v0.32.0: the radial's box no longer passes
    // through verbatim. It goes out through the inverse of Lightroom's
    // frame affine and comes back through the affine, and the WIRE carries
    // six decimals — Lightroom's own precision, which is the precision any
    // value that has ever been through Lightroom actually has. So the
    // corners settle by up to half a wire step, measured here at
    // 4.6 × 10⁻⁷ of the frame = 0.005 px on a 9504 px export. It is a
    // one-time settle onto the wire grid, not a drift: the second round
    // trip is exact, which the re-emit below is what pins.
    let approx = |a: &MaskGeometry, b: &MaskGeometry| match (a, b) {
        (
            MaskGeometry::Radial { top: t1, left: l1, bottom: b1, right: r1, .. },
            MaskGeometry::Radial { top: t2, left: l2, bottom: b2, right: r2, .. },
        ) => [(t1, t2), (l1, l2), (b1, b2), (r1, r2)]
            .iter()
            .all(|(x, y)| (**x - **y).abs() < 1e-6),
        _ => false,
    };
    assert!(approx(&back.masks[1].mask, &expect[1].mask), "{:?}", back.masks[1].mask);
    expect[1].mask = back.masks[1].mask.clone();
    assert_eq!(back.masks, expect);
    // The wire grid is a FIXED POINT, not a ratchet: once a box has been
    // through the projection its own re-emit reproduces it to the bit.
    assert_eq!(
        xmp_to_recipe(&recipe_to_xmp(&back)).masks[1].mask,
        back.masks[1].mask,
        "the second round trip is exact"
    );
    for (i, (was, now)) in r.masks.iter().zip(&back.masks).enumerate() {
        assert_eq!(
            lr_net_inverted(was),
            lr_net_inverted(now),
            "mask {i}: the net inversion is the part that must survive"
        );
    }
}

#[test]
fn bitmap_masks_come_back_only_through_the_payload() {
    // The writer skips raster corrections (no classic-XMP encoding), so the
    // crs reading must return only the parametric mask — never a phantom.
    let mixed = mixed_parametric_and_raster();
    let back = xmp_to_recipe(&bare_document(&mixed, None));
    assert_eq!(back.masks.len(), 1);
    assert_eq!(back.masks[0].mask, mixed.masks[0].mask);
    assert_eq!(back.masks[0].exposure_ev, -1.0);
    // The payload (v1.3.1) is what brings it back — by its bare name, the
    // raster itself being one this test never wrote.
    let whole = xmp_to_recipe(&recipe_to_xmp(&mixed));
    assert_eq!(whole.masks.len(), 2);
    assert_eq!(whole.masks[1].mask, MaskGeometry::Bitmap { path: "subject.png".into() });
    assert_eq!(whole.masks[1].exposure_ev, 0.6);
}

#[test]
fn foreign_as_shot_sidecar_imports_no_wb_and_drops_identity_curves() {
    // A Lightroom-style sidecar (no AutoShade marker): "As Shot" Temperature
    // and Tint are the CAMERA's values, not edits — they must NOT import.
    // LR also always writes the master curve; the 2-point identity means
    // "no curve" and must collapse to empty.
    let lr = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core 7.0-c000">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    crs:WhiteBalance="As Shot"
    crs:Temperature="5150"
    crs:Tint="+10"
    crs:Exposure2012="+0.65"
    crs:Contrast2012="+22"
    crs:Sharpness="40"
    crs:HasSettings="True">
   <crs:ToneCurvePV2012>
    <rdf:Seq>
     <rdf:li>0, 0</rdf:li>
     <rdf:li>255, 255</rdf:li>
    </rdf:Seq>
   </crs:ToneCurvePV2012>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
"#;
    let r = xmp_to_recipe(lr);
    assert_eq!(r.temperature_k, None, "as-shot Kelvin is not an edit");
    assert_eq!(r.tint, 0.0, "as-shot tint is not an edit");
    assert_eq!(r.exposure_ev, 0.65);
    assert_eq!(r.contrast, 22.0);
    // 1:1 since v0.31.1. This is the value the user's own seven reference
    // sidecars carry, and it used to import as 60.
    assert_eq!(r.sharpening, 40.0);
    assert!(r.tone_curve.is_empty(), "identity curve must collapse");
    // A Custom-WB foreign sidecar DOES import its Kelvin + tint.
    let custom = lr.replace("As Shot", "Custom");
    let rc = xmp_to_recipe(&custom);
    assert_eq!(rc.temperature_k, Some(5150.0));
    assert_eq!(rc.tint, 10.0);
}

#[test]
fn xml_values_round_trip_hostile_text_and_foreign_references_exactly_once() {
    let hostile = r#"& < > " ' literal &lt; masks\Bob's "sky".xmp"#;
    let r = EditRecipe {
        rationale: hostile.into(),
        masks: vec![LocalAdjustment { name: hostile.into(), ..Default::default() }],
        ..Default::default()
    };
    let xmp = recipe_to_xmp(&r);
    assert!(xmp.contains("&quot;sky&quot;"), "attribute quotes are escaped: {xmp}");
    let back = xmp_to_recipe(&xmp);
    assert_eq!(back.rationale, hostile);
    assert_eq!(back.masks[0].name, hostile);

    let foreign = r#"<rdf:Description crs:CorrectionName = "Bob&apos;s &#x3C;sky&#62; &#38; &quot;sea&quot;"/>"#;
    assert_eq!(
        Tag::new(foreign).crs_str("CorrectionName").as_deref(),
        Some(r#"Bob's <sky> & "sea""#)
    );
}

#[test]
fn comments_and_whitespace_cannot_hijack_the_crs_description_or_merge() {
    let fake = r#"<!-- <rdf:Description xmlns:crs="urn:fake" crs:Exposure2012="9"/> -->"#;
    let doc = format!(
        "{fake}\n<rdf:Description rdf:about=\"\" \
             xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
             crs:Exposure2012 = \"+0.65\" crs:HasSettings=\"True\">\
             </rdf:Description>"
    );
    assert_eq!(xmp_to_recipe(&doc).exposure_ev, 0.65);
    let merged = merged_doc(
        &doc,
        &EditRecipe { exposure_ev: 0.25, ..Default::default() },
    )
    .expect("the real description is mergeable");
    assert!(merged.contains(fake), "the foreign comment survives verbatim");
    assert_eq!(xmp_to_recipe(&merged).exposure_ev, 0.25);
}

/// R25 P1 rewrote this test's premise. It used to read
/// `partial_and_unsupported_masks_are_not_rendered…` and pin the old
/// all-or-nothing rule: five corrections in, five losses, zero masks. Two
/// of those five carry nothing worse than a rotation angle and a DEFAULT
/// blend mode, which is what every Lightroom radial and every Lightroom
/// component look like — so the rule refused the user's whole catalog.
/// Now the readable ones import with a named note, the genuinely
/// unreadable ones still do not, and the base's block is still preserved
/// byte-for-byte while the develop has not touched it.
#[test]
fn readable_corrections_import_with_a_note_and_their_group_is_still_preserved() {
    let doc = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core">
     <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
      <rdf:Description rdf:about=""
        xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
        crs:HasSettings="True">
       <crs:MaskGroupBasedCorrections>
        <rdf:Seq>
         <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionActive="true">
          <crs:CorrectionMasks><rdf:Seq>
           <rdf:li crs:What="Mask/Gradient" crs:ZeroX="0.5" crs:ZeroY="0.8" crs:FullX="0.5" crs:FullY="0.2"/>
           <rdf:li crs:What="Mask/Brush"/>
          </rdf:Seq></crs:CorrectionMasks>
         </rdf:Description></rdf:li>
         <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionActive="false">
          <crs:CorrectionMasks><rdf:Seq>
           <rdf:li crs:What="Mask/Gradient" crs:ZeroX="0.4" crs:ZeroY="0.8" crs:FullX="0.4" crs:FullY="0.2"/>
          </rdf:Seq></crs:CorrectionMasks>
         </rdf:Description></rdf:li>
         <rdf:li><rdf:Description crs:What="Correction">
          <crs:CorrectionMasks><rdf:Seq>
           <rdf:li crs:What="Mask/CircularGradient" crs:Top="0.2" crs:Left="0.2" crs:Bottom="0.8" crs:Right="0.8" crs:Feather="50" crs:Roundness="0" crs:Flipped="false" crs:Angle="12"/>
          </rdf:Seq></crs:CorrectionMasks>
         </rdf:Description></rdf:li>
         <rdf:li><rdf:Description crs:What="Correction">
          <crs:CorrectionMasks><rdf:Seq>
           <rdf:li crs:What="Mask/Gradient" crs:MaskBlendMode="0" crs:ZeroX="0.3" crs:ZeroY="0.8" crs:FullX="0.3" crs:FullY="0.2"/>
          </rdf:Seq></crs:CorrectionMasks>
         </rdf:Description></rdf:li>
         <rdf:li><rdf:Description crs:What="Correction">
          <crs:CorrectionMasks><rdf:Seq>
           <rdf:li crs:What="Mask/Brush"/>
          </rdf:Seq></crs:CorrectionMasks>
         </rdf:Description></rdf:li>
        </rdf:Seq>
       </crs:MaskGroupBasedCorrections>
      </rdf:Description>
     </rdf:RDF>
    </x:xmpmeta>"#;

    let parsed = xmp_to_recipe(doc);
    assert_eq!(
        parsed.masks.len(),
        2,
        "the rotated radial and the default-blend-mode gradient are readable: {:?}",
        parsed.masks
    );
    assert_eq!(
        unsupported_corrections(doc),
        3,
        "the brush pair and the muted correction are the only refusals"
    );
    let losses = import_losses(doc);
    assert_eq!(losses.len(), 4, "three refusals plus the rotation note: {losses:?}");
    assert_eq!(
        losses.iter().filter(|l| l.reason == MaskImportReason::Rotation(12)).count(),
        1,
        "crs:Angle=\"12\" is named WITH ITS ANGLE, not silently discarded: {losses:?}"
    );
    assert!(
        !losses.iter().any(|l| l.reason == MaskImportReason::BlendMode),
        "the DEFAULT blend mode costs nothing and must raise nothing: {losses:?}"
    );
    // The rotated radial imports UNROTATED — the reading is honest about
    // being approximate, which is the whole point of naming the loss.
    assert!(
        parsed
            .masks
            .iter()
            .any(|m| matches!(m.mask, MaskGeometry::Radial { angle, .. } if angle == 0.0)),
        "crs:Angle is not mapped onto the engine angle in this batch"
    );

    let start = doc.find("<crs:MaskGroupBasedCorrections>").unwrap();
    let end = doc.find("</crs:MaskGroupBasedCorrections>").unwrap()
        + "</crs:MaskGroupBasedCorrections>".len();
    let original = &doc[start..end];
    let merged = merged_doc(
        doc,
        &EditRecipe { exposure_ev: 0.25, ..Default::default() },
    )
    .expect("the surrounding document remains mergeable");
    assert!(merged.contains(original), "the original mask group is retained verbatim");
    assert!(merged.contains("Mask/Brush"));
    assert!(merged.contains(r#"crs:CorrectionActive="false""#));
    assert!(merged.contains(r#"crs:Angle="12""#));
    assert!(merged.contains(r#"crs:MaskBlendMode="0""#));
}

/// L05#4: the preserve rule yields to the recipe's own masks — the save
/// in hand is the newest intent, so the published document carries THIS
/// develop's masks, the foreign block goes, and the loss is a note
/// rather than a silence (before: the output showed an older pass's
/// masks and none of the develop's, reported as plain success).
#[test]
fn a_recipe_with_masks_outranks_the_bases_foreign_mask_block() {
    let doc = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core">
     <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
      <rdf:Description rdf:about=""
        xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
        crs:HasSettings="True">
       <crs:MaskGroupBasedCorrections>
        <rdf:Seq>
         <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionActive="true">
          <crs:CorrectionMasks><rdf:Seq>
           <rdf:li crs:What="Mask/Brush"/>
          </rdf:Seq></crs:CorrectionMasks>
         </rdf:Description></rdf:li>
        </rdf:Seq>
       </crs:MaskGroupBasedCorrections>
      </rdf:Description>
     </rdf:RDF>
    </x:xmpmeta>"#;
    let mut r = EditRecipe { exposure_ev: 0.25, ..Default::default() };
    r.masks.push(LocalAdjustment {
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
        name: "face".into(),
        exposure_ev: 0.4,
        ..Default::default()
    });
    let out = merge_recipe_into_xmp(doc, &r).expect("mergeable");
    assert!(
        out.doc.contains("Mask/CircularGradient"),
        "the develop's own mask is published: {}",
        out.doc
    );
    assert!(!out.doc.contains("Mask/Brush"), "the foreign block is not resurrected");
    assert_eq!(out.notes.len(), 1, "the replacement is disclosed: {:?}", out.notes);
    assert!(
        out.notes[0].contains("1 thing(s)") && out.notes[0].contains("1 edited mask(s)"),
        "the note names both counts: {}",
        out.notes[0]
    );
    // The mirror case stays preserved-without-note: nothing of the user's
    // is suppressed when the recipe has no masks.
    let out2 = merge_recipe_into_xmp(
        doc,
        &EditRecipe { exposure_ev: 0.25, ..Default::default() },
    )
    .expect("mergeable");
    assert!(out2.doc.contains("Mask/Brush"), "no recipe masks → the block is preserved");
    assert!(out2.notes.is_empty(), "a pure preserve has no loss to note: {:?}", out2.notes);
}

/// L05#1: the attribute-carrying spelling of an owned element is the SAME
/// property (legal XML; the writer's strip already matched it by name) —
/// the literal reader missed it, imported "no curve", and the merge then
/// deleted the element from the user's own sidecar with nothing written
/// in its place.
#[test]
fn an_attribute_form_curve_is_read_not_deleted() {
    let doc = r#"<rdf:Description rdf:about=""
        xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
        crs:HasSettings="True">
       <crs:ToneCurvePV2012 xml:lang="x-default"><rdf:Seq>
        <rdf:li>0, 20</rdf:li>
        <rdf:li>255, 240</rdf:li>
       </rdf:Seq></crs:ToneCurvePV2012>
      </rdf:Description>"#;
    let r = xmp_to_recipe(doc);
    assert_eq!(
        r.tone_curve,
        vec![CurvePoint { input: 0, output: 20 }, CurvePoint { input: 255, output: 240 }],
        "the attribute-form curve is read"
    );
    // Merging a NEW curve over it must not leave two curves behind: the
    // attribute-form element is stripped (by name) and ours replaces it.
    let merged = merged_doc(
        doc,
        &EditRecipe {
            tone_curve: vec![
                CurvePoint { input: 0, output: 5 },
                CurvePoint { input: 255, output: 250 },
            ],
            ..Default::default()
        },
    )
    .expect("mergeable");
    assert!(!merged.contains("0, 20"), "the old spelling is stripped: {merged}");
    assert_eq!(xmp_to_recipe(&merged).tone_curve[0].output, 5, "the new curve answers");
}

/// L05#1: an attribute-form mask GROUP is a real group — reading it as
/// "absent" reported zero unsupported corrections AND told the merge it
/// was free to replace the block.
#[test]
fn an_attribute_form_mask_group_counts_as_a_loss_and_survives_the_merge() {
    let doc = r#"<rdf:Description rdf:about=""
        xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
        crs:HasSettings="True">
       <crs:MaskGroupBasedCorrections rdf:parseType="Resource">
        <rdf:Seq>
         <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionActive="true">
          <crs:CorrectionMasks><rdf:Seq>
           <rdf:li crs:What="Mask/Brush"/>
          </rdf:Seq></crs:CorrectionMasks>
         </rdf:Description></rdf:li>
        </rdf:Seq>
       </crs:MaskGroupBasedCorrections>
      </rdf:Description>"#;
    assert_eq!(unsupported_corrections(doc), 1, "the brush correction is a counted loss");
    let merged = merged_doc(
        doc,
        &EditRecipe { exposure_ev: 0.25, ..Default::default() },
    )
    .expect("mergeable");
    assert!(merged.contains("Mask/Brush"), "the group survives the merge: {merged}");
}

/// A whitespace-carrying close tag (`</crs:Key >`) is the same close in
/// XML; the literal close scan ran past it.
#[test]
fn a_close_tag_with_trailing_space_still_ends_a_property_element() {
    let doc = r#"<rdf:Description rdf:about=""
        xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
       <crs:Exposure2012>+0.65</crs:Exposure2012 >
      </rdf:Description>"#;
    assert_eq!(Scope::new(doc).crs_f32("Exposure2012"), Some(0.65));
}

/// Present-but-unreadable is a DISCLOSED loss, not "no curve": the
/// attribute-form spelling used to make the element invisible to the
/// disclosure as well, so bad points imported as a silent neutral.
#[test]
fn an_unreadable_attribute_form_curve_is_named_by_unparsable_crs_numbers() {
    let doc = r#"<rdf:Description rdf:about=""
        xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
       <crs:ToneCurvePV2012 xml:lang="x-default"><rdf:Seq>
        <rdf:li>999, -5</rdf:li>
       </rdf:Seq></crs:ToneCurvePV2012>
      </rdf:Description>"#;
    let bad = unparsable_crs_numbers(doc);
    assert!(bad.iter().any(|v| v == "ToneCurvePV2012"), "disclosed: {bad:?}");
    // An element that never closes is the same disclosed loss.
    let unterminated = r#"<rdf:Description rdf:about=""
        xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
       <crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li></rdf:Seq>
      </rdf:Description>"#;
    let bad = unparsable_crs_numbers(unterminated);
    assert!(bad.iter().any(|v| v == "ToneCurvePV2012"), "disclosed: {bad:?}");
}

/// L05#7: a document binding the camera-raw namespace to another prefix
/// (or `crs` to another URI) is one every scanner here misreads — the
/// merge REFUSES (the caller regenerates and discloses) instead of
/// splicing a second, contradictory settings block beside the foreign
/// one, and the import discloses instead of coming back silently neutral.
#[test]
fn a_foreign_camera_raw_prefix_refuses_the_merge_and_is_disclosed() {
    let doc = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
     <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
      <rdf:Description rdf:about=""
        xmlns:cr="http://ns.adobe.com/camera-raw-settings/1.0/"
        cr:Exposure2012="+1.00" cr:HasSettings="True">
      </rdf:Description>
     </rdf:RDF>
    </x:xmpmeta>"#;
    assert!(
        merge_recipe_into_xmp(doc, &EditRecipe::default()).is_none(),
        "a foreign camera-raw prefix is refused, never duplicated"
    );
    let bad = unparsable_crs_numbers(doc);
    assert_eq!(bad.len(), 1, "one entry naming the binding: {bad:?}");
    assert!(bad[0].contains("`cr:`"), "the prefix is named: {}", bad[0]);

    let crooked = r#"<rdf:Description rdf:about=""
        xmlns:crs="http://example.invalid/ns" crs:Exposure2012="+1.00">
      </rdf:Description>"#;
    assert!(
        merge_recipe_into_xmp(crooked, &EditRecipe::default()).is_none(),
        "a crs prefix bound to a foreign URI is not camera raw"
    );
    assert!(!unparsable_crs_numbers(crooked).is_empty());
}

/// L05#7 sub-item 4: `xmlns:crs` may legally live on an ANCESTOR
/// (`rdf:RDF`) with every setting in property-element form — the
/// attribute-only test missed that Description, and the merge spliced a
/// SECOND settings Description into the same document.
#[test]
fn a_description_whose_crs_children_declare_the_namespace_upstream_is_still_found() {
    let doc = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
     <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
       xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
      <rdf:Description rdf:about="">
       <crs:Exposure2012>+0.80</crs:Exposure2012>
      </rdf:Description>
     </rdf:RDF>
    </x:xmpmeta>"#;
    assert_eq!(xmp_to_recipe(doc).exposure_ev, 0.8, "element-form settings are found");
    let merged = merged_doc(
        doc,
        &EditRecipe { exposure_ev: 0.25, ..Default::default() },
    )
    .expect("mergeable");
    assert_eq!(
        merged.matches("<rdf:Description").count(),
        1,
        "spliced in place, not duplicated: {merged}"
    );
    assert_eq!(xmp_to_recipe(&merged).exposure_ev, 0.25);
    assert!(!merged.contains("+0.80"), "the old element spelling is stripped");
}

/// The guard the refusal gate must not break: a genuinely settings-free
/// ratings sidecar still takes the INSERT path (that path exists because
/// regenerating over one reported an unfixable loss on every save).
#[test]
fn a_ratings_only_sidecar_still_takes_the_insert_path() {
    let doc = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
     <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
      <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/"
        xmp:Rating="4">
      </rdf:Description>
     </rdf:RDF>
    </x:xmpmeta>"#;
    let out = merge_recipe_into_xmp(
        doc,
        &EditRecipe { exposure_ev: 0.25, ..Default::default() },
    )
    .expect("insertable");
    assert!(out.doc.contains(r#"xmp:Rating="4""#), "the rating survives verbatim");
    assert_eq!(xmp_to_recipe(&out.doc).exposure_ev, 0.25, "our settings are added");
    assert!(out.notes.is_empty(), "a clean insert has no loss: {:?}", out.notes);
}

#[test]
fn xmp_input_is_bounded_and_numeric_groups_follow_recipe_boundaries() {
    let doc = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core">
     <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
      <rdf:Description rdf:about=""
        xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
        crs:WhiteBalance="Custom"
        crs:Temperature="90000"
        crs:Exposure2012="99"
        crs:Contrast2012="-500"
        crs:Sharpness="200"
        crs:HasCrop="True"
        crs:CropLeft="-1"
        crs:CropTop="0"
        crs:CropRight="1"
        crs:CropBottom="1"
        crs:HasSettings="True">
       <crs:ToneCurvePV2012><rdf:Seq>
        <rdf:li>999, -5</rdf:li>
       </rdf:Seq></crs:ToneCurvePV2012>
       <crs:ToneCurvePV2012Red><rdf:Seq>
        <rdf:li>broken</rdf:li>
       </rdf:Seq></crs:ToneCurvePV2012Red>
       <crs:MaskGroupBasedCorrections><rdf:Seq>
        <rdf:li><rdf:Description crs:What="Correction" crs:LocalExposure2012="9">
         <crs:CorrectionMasks><rdf:Seq>
          <rdf:li crs:What="Mask/Gradient" crs:ZeroX="0.5" crs:ZeroY="0.8" crs:FullX="0.5" crs:FullY="0.2"/>
         </rdf:Seq></crs:CorrectionMasks>
        </rdf:Description></rdf:li>
       </rdf:Seq></crs:MaskGroupBasedCorrections>
      </rdf:Description>
     </rdf:RDF>
    </x:xmpmeta>"#;

    let r = xmp_to_recipe(doc);
    assert_eq!(r.temperature_k, Some(40000.0));
    assert_eq!(r.exposure_ev, 5.0);
    assert_eq!(r.contrast, -100.0);
    assert_eq!(r.sharpening, 150.0);
    assert_eq!(r.crop, None, "invalid compound crop geometry is rejected");
    assert!(
        r.tone_curve.is_empty(),
        "out-of-domain curve coordinates are rejected as a group — the old \
             saturation policy imported '999, -5' as a near-black one-point curve"
    );
    assert!(r.red_curve.is_empty(), "a malformed curve is rejected as a group");
    assert!(r.masks.is_empty(), "an out-of-range local correction is rejected as partial");
    assert_eq!(unsupported_corrections(doc), 1);

    let bad = unparsable_crs_numbers(doc);
    for key in [
        "Temperature",
        "Exposure2012",
        "Contrast2012",
        "Sharpness",
        "CropLeft",
        "ToneCurvePV2012Red",
    ] {
        assert!(bad.iter().any(|v| v == key), "{key} must be disclosed: {bad:?}");
    }

    let oversized = "x".repeat(MAX_XMP_BYTES + 1);
    assert!(crs_own_scope(&oversized).is_empty());
    assert_eq!(xmp_to_recipe(&oversized), EditRecipe::default());
    assert!(merged_doc(&oversized, &EditRecipe::default()).is_none());
    assert_eq!(
        unparsable_crs_numbers(&oversized),
        vec!["XMP document exceeds the 16 MiB limit".to_string()]
    );
}

// --- R27 Batch-5: the `Mask/Image` AI arm (L-08 Arm C) -------------------
//
// Fixtures below reproduce the shape measured across 105 real `Mask/Image`
// instances in the user's library on 2026-08-19: 21 distinct attribute
// names, `MaskActive="true"` on all of them, `MaskVersion="1"` on all of
// them, `MaskSubType` in {0, 1, 2}, `ReferencePoint` on all of them, and
// exactly one optional child element (`crs:Gesture`, on 40).

/// One `Mask/Image` component. `extra` splices additional attributes;
/// `gesture` splices a `crs:Gesture` child (empty = self-closing, which is
/// what 65 of the 105 real instances are).
fn lr_ai_mask(subtype: &str, blend: &str, value: &str, extra: &str, gesture: &str) -> String {
    let head = format!(
        "        <rdf:Description\n\
             \x20        crs:What=\"Mask/Image\"\n\
             \x20        crs:MaskActive=\"true\"\n\
             \x20        crs:MaskName=\"Sky 1\"\n\
             \x20        crs:MaskBlendMode=\"{blend}\"\n\
             \x20        crs:MaskInverted=\"false\"\n\
             \x20        crs:MaskSyncID=\"440777CD3CB8E24BB8E16028893B45DC\"\n\
             \x20        crs:MaskValue=\"{value}\"\n\
             \x20        crs:MaskVersion=\"1\"\n\
             \x20        crs:MaskSubType=\"{subtype}\"\n\
             \x20        crs:ReferencePoint=\"0.605469 0.281525\"{extra}"
    );
    if gesture.is_empty() {
        format!("       <rdf:li>\n{head}/>\n       </rdf:li>\n")
    } else {
        format!(
            "       <rdf:li>\n{head}>\n\
                 \x20       <crs:Gesture>\n\
                 \x20        <rdf:Seq>\n\
                 {gesture}\
                 \x20        </rdf:Seq>\n\
                 \x20       </crs:Gesture>\n\
                 \x20       </rdf:Description>\n\
                 \x20      </rdf:li>\n"
        )
    }
}

/// The provenance block Lightroom writes on a real sky mask — every
/// attribute this engine carries and never interprets.
const LR_AI_PROVENANCE: &str = "\n         crs:InputDigest=\"D0DAC04EB58F013F49D93EF47D22794E\"\
\n         crs:InputDigestVersion=\"2\"\
\n         crs:MaskDigest=\"00D1A1B68591DF41F6CA3F8F805D0F1B\"\
\n         crs:WholeImageArea=\"0/1,0/1,1920/1,2880/1\"\
\n         crs:Origin=\"0,0\"\
\n         crs:ModelVersion=\"234881976\"";

/// THE DOMINANT REFUSAL, closed. Before R27 Batch-5 a correction holding a
/// `Mask/Image` was thrown away entire — 78 corrections across 40 files,
/// 40 % of every file in the reference library that has a mask at all —
/// and it took the gradient standing beside it with it.
///
/// MUTATION-LINED. Verified red by reverting the `"Mask/Image"` arm of
/// `classify_correction` to `unknown_component = true` (transcript in the
/// batch report): the correction disappears and the gradient with it.
#[test]
fn an_ai_mask_imports_beside_the_shapes_it_used_to_take_down() {
    let doc = lr_doc(&lr_correction(
        "Mask 1",
        "",
        &format!("{}{}", lr_gradient("0"), lr_ai_mask("2", "0", "1", LR_AI_PROVENANCE, "")),
    ));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "the correction must import: {:?}", r.masks);
    let m = &r.masks[0];
    // The parametric shape is still the BASE — an AI mask does not displace
    // a gradient that was there first (`base_geometry_at`).
    assert!(matches!(m.mask, MaskGeometry::Linear { .. }), "{:?}", m.mask);
    assert_eq!(m.components.len(), 1, "the AI mask rides as a component");
    assert_eq!(m.components[0].mode, MaskCombine::Add, "MaskBlendMode=0 is a union");
    let MaskGeometry::AiMask {
        name,
        subtype,
        ref_x,
        ref_y,
        blend_mode,
        value,
        inverted,
        mask_version,
        provenance,
        gesture,
        raster,
    } = &m.components[0].geometry
    else {
        panic!("expected an AI mask, got {:?}", m.components[0].geometry);
    };
    assert_eq!(name.as_str(), "Sky 1");
    assert_eq!((*subtype, *blend_mode, *value, *inverted, *mask_version), (2, 0, 1.0, false, 1));
    assert_eq!((*ref_x, *ref_y), (0.605469, 0.281525), "the click arrives verbatim");
    assert!(gesture.is_empty(), "no crs:Gesture on this fixture");
    // NOTHING is resolved at parse time: importing a library must not spawn
    // a model run per photo.
    assert!(raster.is_none(), "the alpha is recomputed at DEVELOP time, not here");
    // Every provenance attribute, in document order, carried and untouched.
    let keys: Vec<&str> = provenance.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        keys,
        [
            "InputDigest",
            "InputDigestVersion",
            "MaskDigest",
            "WholeImageArea",
            "Origin",
            "ModelVersion"
        ]
    );
    assert_eq!(provenance[3].1, "0/1,0/1,1920/1,2880/1");

    // And the import SAYS what kind of thing arrived — a RE-DERIVATION,
    // not Adobe's raster. Importing this silently would be worse than the
    // refusal it replaced.
    let losses = import_losses(&doc);
    assert!(
        losses.iter().any(|l| l.reason == MaskImportReason::AiMaskRecomputed),
        "a re-derived AI mask must be disclosed: {losses:?}"
    );
    let line = describe_import_losses(1, &losses).unwrap_or_default();
    assert!(
        line.contains("re-derived") && line.contains("Adobe"),
        "the prose must name the recomputation, not just the count: {line}"
    );
}

/// A correction whose ONLY component is an AI mask imports too, with the
/// AI mask as its base — the 59 corrections in the census that carry
/// nothing else.
#[test]
fn an_ai_only_correction_takes_the_ai_mask_as_its_base() {
    let doc = lr_doc(&lr_correction(
        "Mask 2",
        "",
        &lr_ai_mask("0", "0", "1", LR_AI_PROVENANCE, ""),
    ));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "an AI-only correction imports: {:?}", r.masks);
    assert!(matches!(r.masks[0].mask, MaskGeometry::AiMask { subtype: 0, .. }));
    assert!(r.masks[0].components.is_empty(), "the base is not also a component");
}

/// The `crs:Gesture` child — the photographer's brush refinement of the AI
/// mask (40 of 105 instances) — is carried, so the corrections whose only
/// brush content is a gesture arrive whole.
#[test]
fn an_ai_masks_gesture_strokes_are_carried() {
    let doc = lr_doc(&lr_correction(
        "Mask 3",
        "",
        &lr_ai_mask("2", "0", "1", LR_AI_PROVENANCE, &lr_paint_specimen()),
    ));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "{:?}", r.masks);
    let MaskGeometry::AiMask { gesture, .. } = &r.masks[0].mask else {
        panic!("expected an AI mask, got {:?}", r.masks[0].mask);
    };
    assert_eq!(gesture.len(), 1, "one Mask/Paint per Gesture, as measured");
    assert_eq!(gesture[0].value, 0.439815);
    assert_eq!(gesture[0].radius, 0.582157);
    assert!(gesture[0].dabs.starts_with("r 0.581835\nd 0.000684 0.940004"));
}

/// The write-back re-emits the component shape Lightroom wrote — the
/// intent verbatim, so Lightroom rebuilds ITS own alpha from it — with a
/// FRESH `crs:MaskSyncID` per this writer's rule.
///
/// MUTATION-LINED. Verified red by dropping the `ai_mask_xml` routing from
/// `masks_xml`'s base arm (the correction then exports as a bitmap-skip
/// and the whole mask disappears from the sidecar) — transcript in the
/// batch report.
#[test]
fn an_ai_mask_round_trips_back_into_the_sidecar() {
    let doc = lr_doc(&lr_correction(
        "Mask 4",
        "",
        &lr_ai_mask("2", "0", "1", LR_AI_PROVENANCE, ""),
    ));
    let r = xmp_to_recipe(&doc);
    let out = recipe_to_xmp(&r);
    assert!(out.contains(r#"crs:What="Mask/Image""#), "the component kind rides out:\n{out}");
    assert!(out.contains(r#"crs:MaskSubType="2""#), "the intent rides out:\n{out}");
    assert!(
        out.contains(r#"crs:ReferencePoint="0.605469 0.281525""#),
        "the click rides out at the file's own precision:\n{out}"
    );
    assert!(out.contains(r#"crs:MaskVersion="1""#), "the schema stamp rides out:\n{out}");
    for (k, v) in [
        ("InputDigest", "D0DAC04EB58F013F49D93EF47D22794E"),
        ("MaskDigest", "00D1A1B68591DF41F6CA3F8F805D0F1B"),
        ("WholeImageArea", "0/1,0/1,1920/1,2880/1"),
        ("ModelVersion", "234881976"),
    ] {
        assert!(
            out.contains(&format!("crs:{k}=\"{v}\"")),
            "provenance {k} must ride out unchanged:\n{out}"
        );
    }
    // FRESH SyncID — a sidecar we rewrite is OUR document, and this writer
    // mints identities for every component it emits.
    assert!(
        !out.contains("440777CD3CB8E24BB8E16028893B45DC"),
        "the file's own MaskSyncID must not be republished:\n{out}"
    );
    // And the EXPORT discloses the other direction of the same gap.
    let losses = mask_export_losses(&r);
    assert!(
        losses.iter().any(|l| l.reason == MaskLossReason::AiMaskRecomputed),
        "the export must say the pixels shown were not Adobe's: {losses:?}"
    );
}

/// The zoned reverse-fit's SKY zone rides out as LIGHTROOM'S OWN Select
/// Sky, and the land zone as that mask inverted.
///
/// What this replaces: both zones were `MaskGeometry::Bitmap`, which
/// classic ACR XMP has no encoding for at all, so `masks_xml` skipped the
/// whole correction with a named `MaskLossReason::Bitmap`. The sidecar
/// carried the global fit and the two edits that actually separate a
/// repainted sky from its ground never reached Lightroom. A `Mask/Image`
/// component with `crs:MaskSubType="2"` carries the same INTENT in a form
/// Lightroom rebuilds its own sky alpha from.
///
/// The honesty is unchanged and is asserted here: the pixels this app
/// showed came from OUR render, never Adobe's raster, which is exactly
/// what `MaskLossReason::AiMaskRecomputed` says — and it REPLACES the
/// `Bitmap` loss rather than joining it, because nothing was left out.
/// Recolour gains remain engine-only and keep their own loss.
///
/// MUTATION-LINED: put `sky_attachment`/`land_attachment` in `fit_zoned`
/// back on `MaskGeometry::Bitmap` and the first half fails; drop the
/// `AiMaskRecomputed` push from `masks_xml` and the disclosure half does.
#[test]
fn a_reverse_fit_zone_round_trips_select_sky_role_and_inversion_home() {
    // ONE home for the inversion, exactly as `fit_zoned` builds it: the
    // CORRECTION carries the flag and the component carries `false`, so
    // `lr_net_inverted` is what reaches `crs:MaskInverted`.
    let zone = |inverted: bool, gains: Option<[f32; 3]>| EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::select_sky(0.5, 0.25, false, "mask-zone-sky.png".into()),
            role: if inverted {
                crate::recipe::MaskRole::ZoneLand
            } else {
                crate::recipe::MaskRole::ZoneSky
            },
            inverted,
            exposure_ev: -0.4,
            saturation: 12.0,
            color_gains: gains,
            ..Default::default()
        }],
        ..Default::default()
    };

    let r = zone(false, None);
    let (out, losses) = recipe_to_xmp_with_losses(&r);
    assert_eq!(
        out.matches(r#"crs:What="Mask/Image""#).count(),
        1,
        "exactly one Select Sky component reaches the sidecar:\n{out}"
    );
    assert!(out.contains(r#"crs:MaskSubType="2""#), "…and it is the SKY subtype:\n{out}");
    assert!(
        out.contains(r#"crs:ReferencePoint="0.5 0.25""#),
        "…prompted at the alpha's own centre of mass:\n{out}"
    );
    assert!(out.contains(r#"crs:MaskInverted="false""#), "…upright:\n{out}");
    assert!(out.contains(r#"crs:MaskVersion="1""#), "…with Lightroom's schema stamp:\n{out}");
    // The CORRECTION, which is the whole point: the zone's dials are in
    // the file now instead of being skipped along with the mask.
    assert!(
        out.contains(&format!("crs:LocalExposure2012=\"{}\"", local_fmt(-0.4 / 4.0))),
        "the zone's exposure rides out:\n{out}"
    );
    assert!(
        out.contains(&format!("crs:LocalSaturation=\"{}\"", local_fmt(12.0 / 100.0))),
        "…and its saturation:\n{out}"
    );
    // The disclosure: re-derived, NOT a dropped bitmap.
    assert!(
        losses.iter().any(|l| l.reason == MaskLossReason::AiMaskRecomputed),
        "the alpha shown here is ours, and the export must say so: {losses:?}"
    );
    assert!(
        !losses.iter().any(|l| l.reason == MaskLossReason::Bitmap),
        "nothing was left out of the sidecar, so there is no bitmap loss: {losses:?}"
    );

    // Recolour gains are still engine-only and keep their own named loss —
    // the carrier change did not quietly widen what XMP can hold.
    let recoloured = mask_export_losses(&zone(false, Some([1.18, 0.96, 0.85])));
    assert!(
        recoloured.iter().any(|l| l.reason == MaskLossReason::Recolour)
            && recoloured.iter().any(|l| l.reason == MaskLossReason::AiMaskRecomputed),
        "both sentences, neither swallowing the other: {recoloured:?}"
    );

    // THE ROUND TRIP, through this app's own reader: the writer must emit
    // only what the parser's closed vocabulary accepts, or a sidecar we
    // wrote would fail to re-import — which is how a photographer loses a
    // mask to their own save.
    for inverted in [false, true] {
        let doc = recipe_to_xmp(&zone(inverted, None));
        // The sidecar carries the NET on the component, which is the only
        // place Lightroom has for it — the land zone's `crs:MaskInverted`
        // is `"true"` even though its geometry's own bit is `false`.
        assert!(
            doc.contains(&format!("crs:MaskInverted=\"{inverted}\"")),
            "inverted={inverted}: the net must reach the component:\n{doc}"
        );
        let back = xmp_to_recipe(&doc);
        assert_eq!(back.masks[0].role, if inverted { crate::recipe::MaskRole::ZoneLand } else { crate::recipe::MaskRole::ZoneSky });
        assert_eq!(back.masks.len(), 1, "inverted={inverted}: {:?}", back.masks);
        let MaskGeometry::AiMask {
            subtype,
            ref_x,
            ref_y,
            inverted: geom_inv,
            mask_version,
            ..
        } = &back.masks[0].mask
        else {
            panic!("inverted={inverted}: expected the Select Sky component back");
        };
        assert_eq!(
            (*subtype, *ref_x, *ref_y, *geom_inv, *mask_version),
            (2, 0.5, 0.25, false, 1),
            "inverted={inverted}: subtype, click and polarity all survive"
        );
        // Native CRS carries the net bit on the base geometry. Consistent
        // editor metadata restores its original home on the correction;
        // it must neither duplicate nor lose that inversion. The engine's
        // net remains the same fact after the trip.
        assert!(
            back.masks[0].inverted == inverted,
            "inverted={inverted}: the authored inversion home survives"
        );
        assert_eq!(
            back.masks[0].net_inverted(),
            inverted,
            "inverted={inverted}: the net is what round-trips"
        );
        assert_eq!(
            back.masks[0].exposure_ev, -0.4,
            "inverted={inverted}: and so does the correction"
        );
    }
}

/// An attribute outside the measured vocabulary is REFUSED, not carried.
/// A name we have never seen means a writer we have not measured (the
/// roundness rule) — and an open-ended attribute bag read off disk and
/// written back into XML is an injection surface besides.
///
/// MUTATION-LINED. Verified red by replacing `parse_ai_mask`'s
/// `_ => return Err(())` arm with `_ => {}` (transcript in the batch
/// report): the unknown attribute is silently dropped and the correction
/// imports as if the file had said nothing surprising.
#[test]
fn an_unmeasured_attribute_on_an_ai_mask_is_refused_not_guessed() {
    let good = lr_doc(&lr_correction("ok", "", &lr_ai_mask("2", "0", "1", "", "")));
    assert_eq!(xmp_to_recipe(&good).masks.len(), 1, "the baseline imports");

    for extra in [
        "\n         crs:SomethingNobodyMeasured=\"1\"",
        // A subtype outside {0,1,2} has no backend, and guessing one would
        // invent a selection.
        "",
    ] {
        let doc = if extra.is_empty() {
            lr_doc(&lr_correction("bad", "", &lr_ai_mask("7", "0", "1", "", "")))
        } else {
            lr_doc(&lr_correction("bad", "", &lr_ai_mask("2", "0", "1", extra, "")))
        };
        assert!(
            xmp_to_recipe(&doc).masks.is_empty(),
            "a Mask/Image outside the measured encoding must not import ({extra:?})"
        );
        let losses = import_losses(&doc);
        assert!(
            losses.iter().any(|l| l.reason.is_drop()),
            "and the refusal must be NAMED: {losses:?}"
        );
    }
}

/// `MaskBlendMode="1"` + `MaskValue="0"` is Lightroom's SUBTRACT pair, not
/// a muted mask — the same reading v0.31.1 taught this parser for
/// parametric components, mapped once through `brush_combine`.
#[test]
fn an_ai_masks_subtract_pair_reads_as_a_subtraction() {
    let doc = lr_doc(&lr_correction(
        "Mask 5",
        "",
        &format!("{}{}", lr_gradient("0"), lr_ai_mask("1", "1", "0", "", "")),
    ));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "{:?}", r.masks);
    assert_eq!(r.masks[0].components.len(), 1);
    assert_eq!(
        r.masks[0].components[0].mode,
        MaskCombine::Subtract,
        "MaskBlendMode=1 carves out"
    );
    let MaskGeometry::AiMask { blend_mode, value, .. } = &r.masks[0].components[0].geometry
    else {
        panic!("expected an AI mask");
    };
    // The pair is CARRIED verbatim for the writer even though the render
    // reads the projected `MaskCombine` — two spellings of one fact.
    assert_eq!((*blend_mode, *value), (1, 0.0));
}

// ================================================================
// R28 Batch-5 5d — THE FOUR ADVERSARIAL SCOPE FIXTURES (F4 A–D)
//
// All four are documents no Lightroom writes; the adjudication rated the
// whole finding "mechanism real, zero sites reachable from real LR". They
// are here because the DEFENCE used to be a coincidence — real Lightroom
// happens not to put these names in these places — and a coincidence is not
// a guard. The typed scope (`Tag` / `Scope`) plus the two narrowed searches
// make them refusals by construction, and these four fixtures say so.
// ================================================================

/// A correction with NO `crs:LocalExposure2012` of its own, one gradient
/// component, and whatever `extra` / `stray` the caller plants.
///
/// `extra` goes on the COMPONENT (inside `crs:CorrectionMasks`); `stray` is
/// spliced in as a child of the correction itself, before the component
/// list. Hand-written rather than built from `lr_correction` deliberately:
/// the point of A is a correction that OMITS a slider, and the Lightroom
/// fixture writes every one of them.
///
/// Authored by US (`x:xmptk="AutoShade 2"`), which matters for exactly one
/// thing: `component_import_reasons` accepts a `Mask/RangeMask` only on our
/// own documents (someone else's range encoding is not ours to interpret).
/// A Lightroom-authored fixture would drop the range for THAT reason and
/// the B control below could not tell the two refusals apart.
fn scope_bleed_doc(extra: &str, stray: &str) -> String {
    format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"AutoShade 2\">\n\
             <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
             <rdf:Description rdf:about=\"\"\n\
             \x20 xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n\
             \x20 crs:HasSettings=\"True\">\n\
             \x20<crs:MaskGroupBasedCorrections><rdf:Seq>\n\
             \x20 <rdf:li><rdf:Description crs:What=\"Correction\"\n\
             \x20  crs:CorrectionAmount=\"1\" crs:CorrectionActive=\"true\"\n\
             \x20  crs:CorrectionName=\"Mask 1\" crs:LocalContrast2012=\"0.2\">\n\
             {stray}\
             \x20  <crs:CorrectionMasks><rdf:Seq>\n\
             \x20   <rdf:li crs:What=\"Mask/Gradient\" crs:MaskActive=\"true\"\n\
             \x20    crs:MaskBlendMode=\"0\" crs:MaskInverted=\"false\" crs:MaskValue=\"1\"\n\
             \x20    crs:ZeroX=\"0.5\" crs:ZeroY=\"0.8\" crs:FullX=\"0.5\" crs:FullY=\"0.2\"{extra}/>\n\
             \x20  </rdf:Seq></crs:CorrectionMasks>\n\
             \x20 </rdf:Description></rdf:li>\n\
             \x20</rdf:Seq></crs:MaskGroupBasedCorrections>\n\
             </rdf:Description></rdf:RDF></x:xmpmeta>\n"
    )
}

/// **F4 SYMPTOM A** — a slider name on a NESTED component must not answer
/// for the correction.
///
/// The correction states no `crs:LocalExposure2012`; its gradient component
/// carries one, at a value (`9`, i.e. +36 EV on the ×4 file scale) far
/// outside Lightroom's own slider. The pre-5d reader scanned the whole
/// correction segment for every slider, so it found the component's number
/// and REFUSED the entire correction as out-of-model — the photographer
/// lost a mask because of an attribute on a shape.
///
/// The nearest real threat this closes: Lightroom really does write
/// `crs:Local*` NAMES on nested components (`LocalInputDigest` and friends,
/// 105 measured instances). They are strings nobody parses as numbers
/// today, which is why nothing has caught fire — a coincidence, now a
/// guard.
///
/// MUTATION: hand `correction_value_reasons` / `parse_one_correction` the
/// whole `seg` again instead of `own`, and the first assertion goes red.
#[test]
fn a_nested_components_slider_name_cannot_answer_for_the_correction() {
    let doc = scope_bleed_doc(" crs:LocalExposure2012=\"9\"", "");
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "the correction must still import: {:?}", r.masks);
    assert_eq!(
        r.masks[0].exposure_ev, 0.0,
        "the correction states no exposure; the component's number is not its value"
    );
    // The correction's OWN sliders still arrive — this is a narrowing, not
    // a blindfold.
    assert_eq!(r.masks[0].contrast, 20.0, "crs:LocalContrast2012=0.2 → +20");
    assert_eq!(unsupported_corrections(&doc), 0, "and nothing was dropped");
}

/// **F4 SYMPTOM B** — a `Mask/RangeMask` outside `crs:CorrectionMasks` is
/// not this correction's range.
///
/// The stray element below sits inside the correction but outside its
/// component list, so no component walk ever counts it — which is exactly
/// why attaching it was SILENT: `range_count` stayed 0, so the
/// `ForeignRangeMask` disclosure could not fire either. The old search was
/// a first-occurrence scan over the whole segment, and the reader then ran
/// from that offset to the END of the segment, so even the colour arm's
/// `rdf:li` could come from an unrelated component.
///
/// MUTATION: restore `Scope::new(seg).find_value_at("What",
/// "Mask/RangeMask")` + `&seg[p..]` and the range comes back.
#[test]
fn a_range_mask_outside_the_component_list_is_not_attached() {
    let stray = "\x20 <rdf:li crs:What=\"Mask/RangeMask\" crs:MaskActive=\"true\"\n\
                     \x20  crs:MaskBlendMode=\"1\" crs:MaskValue=\"0\" crs:MaskInverted=\"true\"\n\
                     \x20  crs:LumRange=\"0 0.2 0.8 1\"/>\n";
    let doc = scope_bleed_doc("", stray);
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "the correction still imports: {:?}", r.masks);
    assert!(
        r.masks[0].range.is_none(),
        "a range mask outside the component list is not this correction's: {:?}",
        r.masks[0].range
    );
    // …and the control: the SAME range component, INSIDE the list, is.
    let inside = scope_bleed_doc("", "").replace(
        "</rdf:Seq></crs:CorrectionMasks>",
        "<rdf:li crs:What=\"Mask/RangeMask\" crs:MaskActive=\"true\" \
             crs:MaskBlendMode=\"1\" crs:MaskValue=\"0\" crs:MaskInverted=\"true\" \
             crs:LumRange=\"0 0.2 0.8 1\"/></rdf:Seq></crs:CorrectionMasks>",
    );
    let r = xmp_to_recipe(&inside);
    assert!(
        matches!(r.masks[0].range, Some(RangeMask::Luminance { .. })),
        "an in-list range component must still be read, or this test proves nothing: {:?}",
        r.masks[0].range
    );
}

/// **F4 SYMPTOM C** — single-quoted attributes are legal XML, and the AI
/// mask's closed-vocabulary gate has to run on them.
///
/// `crs_attributes` hand-rolled its own lexer and looked for a DOUBLE
/// quote, so on this document it found none and returned an empty list.
/// Two consequences, both silent: the eleven provenance / digest keys were
/// dropped on write-back (Lightroom's own recompute ledger, gone from a
/// file we had just accepted), and the refusal loop that is supposed to
/// reject an unmeasured attribute name never looked at anything.
///
/// MUTATION: restore the `find('"')` lexer — the first half loses the
/// digests, the second half stops refusing.
#[test]
fn single_quoted_attributes_carry_provenance_and_still_refuse_the_unknown() {
    let ai = |extra: &str| {
        format!(
            "       <rdf:li><rdf:Description crs:What='Mask/Image' crs:MaskActive='true'\n\
                 \x20        crs:MaskName='Sky 1' crs:MaskBlendMode='0' crs:MaskInverted='false'\n\
                 \x20        crs:MaskSyncID='440777CD3CB8E24BB8E16028893B45DC' crs:MaskValue='1'\n\
                 \x20        crs:MaskVersion='1' crs:MaskSubType='2'\n\
                 \x20        crs:ReferencePoint='0.605469 0.281525'\n\
                 \x20        crs:InputDigest='D0DAC04EB58F013F49D93EF47D22794E'\n\
                 \x20        crs:ModelVersion='234881976'{extra}/></rdf:li>\n"
        )
    };
    let doc = lr_doc(&lr_correction("Mask 1", "", &ai("")));
    let r = xmp_to_recipe(&doc);
    assert_eq!(r.masks.len(), 1, "a single-quoted AI mask imports: {:?}", r.masks);
    let MaskGeometry::AiMask { provenance, .. } = &r.masks[0].mask else {
        panic!("expected an AI mask, got {:?}", r.masks[0].mask);
    };
    assert_eq!(
        provenance.len(),
        2,
        "both provenance keys must be CARRIED, not silently dropped: {provenance:?}"
    );
    // …and they come back out, which is the loss the photographer feels.
    let out = recipe_to_xmp(&r);
    assert!(
        out.contains("crs:InputDigest=\"D0DAC04EB58F013F49D93EF47D22794E\"")
            && out.contains("crs:ModelVersion=\"234881976\""),
        "the digests must survive the round trip:\n{out}"
    );
    // The refusal loop runs on legal XML now: an unmeasured name costs the
    // correction, exactly as it does with double quotes.
    let bogus = lr_doc(&lr_correction("Mask 1", "", &ai(" crs:Bogus='1'")));
    assert!(
        xmp_to_recipe(&bogus).masks.is_empty(),
        "an attribute outside the measured vocabulary must still refuse"
    );
}

/// **F4 SYMPTOM D** — the declared frame comes from ONE `rdf:Description`.
///
/// Width, length and orientation used to be three independent
/// first-occurrence searches over the whole document, so a packet carrying
/// a `tiff:ImageWidth` in one element and the real pair in another produced
/// a frame no element declares — and that frame is the coordinate system
/// every mask and crop decode folds pixel geometry with (`lr_to_engine`).
///
/// MUTATION: make [`FrameScope::resolve`] return `FrameScope(doc)`
/// unconditionally — i.e. point the three reads back at the whole document
/// — and the frame becomes 6000 × 6336, which this file never states.
#[test]
fn the_declared_frame_comes_from_one_description() {
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
             <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
             <rdf:Description rdf:about=\"\" xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\"\n\
             \x20 tiff:ImageWidth=\"6000\"/>\n\
             <rdf:Description rdf:about=\"\" xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\"\n\
             \x20 tiff:ImageWidth=\"9504\" tiff:ImageLength=\"6336\" tiff:Orientation=\"1\"/>\n\
             </rdf:RDF></x:xmpmeta>";
    let frame = FrameAspect::from_xmp(doc).expect("the second Description declares a frame");
    assert_eq!(
        (frame.w, frame.h),
        (9504.0, 6336.0),
        "both dimensions must come from the element that declares both"
    );
    // A document with no `rdf:Description` at all still reads: the
    // narrowing has nothing to protect in a bare fragment, and fixtures in
    // this module that hand `from_xmp` a snippet rely on it.
    let bare = "tiff:ImageWidth=\"800\" tiff:ImageLength=\"600\"";
    let frame = FrameAspect::from_xmp(bare).expect("a bare fragment still declares a frame");
    assert_eq!((frame.w, frame.h), (800.0, 600.0));
}

// ================================================================
// R29-2 — THE FOUR ADVERSARIAL FRAME-SCOPE FIXTURES
//
// R28 Batch-5 5d typed the `crs:` reader's scope and left the `tiff:` frame
// family on a bare-`&str` `declared_number` whose scope was a per-call-site
// CONVENTION. `declared_number` is a method on [`FrameScope`] now, so the
// question can only be asked of a span [`FrameScope::resolve`] produced.
//
// Two of the four pin FALLBACKS, not guarantees. `resolve` deliberately
// widens to the whole document in the two cases its own doc comment names,
// and both are behaviour a photographer's file can reach; they are fixtures
// so that a later narrowing has to face them instead of changing the frame
// — the coordinate system every mask and crop decode folds pixel geometry
// with — by accident. None of these four documents is one Lightroom writes.
// ================================================================

/// **A** — HALF a frame in each of two `rdf:Description`s.
///
/// One element declares only `tiff:ImageWidth`, the next only
/// `tiff:ImageLength`. No element declares both, so `resolve` falls back to
/// the whole document and the pair IS assembled across two elements — the
/// pairing F4 symptom D removes when some element declares both, and which
/// this document gives the reader no way to avoid. PINNED, not endorsed:
/// the alternative is dropping the frame for files nobody has measured.
///
/// MUTATION: drop the fallback (`resolve` returning the last candidate span
/// instead of `doc`) and `from_xmp` returns `None` — the half-declaration
/// the narrowed span sees is not a frame, which the control below states
/// directly.
#[test]
fn half_a_frame_in_each_description_falls_back_to_the_whole_document() {
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
             <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
             <rdf:Description rdf:about=\"\" xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\"\n\
             \x20 tiff:ImageWidth=\"9504\"/>\n\
             <rdf:Description rdf:about=\"\" xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\"\n\
             \x20 tiff:ImageLength=\"6336\" tiff:Orientation=\"1\"/>\n\
             </rdf:RDF></x:xmpmeta>";
    let frame = FrameAspect::from_xmp(doc).expect("the whole-document fallback still reads");
    assert_eq!(
        (frame.w, frame.h),
        (9504.0, 6336.0),
        "the documented fallback assembles the pair across the two elements"
    );
    // CONTROL: half a declaration is not a frame. Neither element on its
    // own answers, which is what makes the assertion above a statement
    // about the FALLBACK rather than about either span.
    let half = "<rdf:Description rdf:about=\"\" tiff:ImageWidth=\"9504\"/>";
    assert!(
        FrameAspect::from_xmp(half).is_none(),
        "a width with no length declares no rectangle"
    );
}

/// **B** — the two XMP spellings MIXED inside one `rdf:Description`.
///
/// `tiff:ImageWidth` is an attribute on the start tag (what Lightroom
/// writes); `tiff:ImageLength` and `tiff:Orientation` are property elements
/// in the body (the same properties' other legal spelling). The scope runs
/// from the element's `<` to the end of its body precisely so that one
/// element declaring both — in either spelling, or one of each — counts as
/// declaring both.
///
/// The first Description is a DECOY carrying a lone `tiff:ImageWidth="6000"`:
/// if the narrowing stopped working the whole-document fallback would read
/// its 6000 first and the frame would be 6000 × 6336.
///
/// MUTATION: make `resolve` end the span at the start tag's `>` (a `Tag`,
/// not a scope) and the mixed element stops declaring a length, so the
/// decoy's 6000 wins.
#[test]
fn the_frame_scope_sees_both_xmp_spellings_of_one_element() {
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
             <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
             <rdf:Description rdf:about=\"\" xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\"\n\
             \x20 tiff:ImageWidth=\"6000\"/>\n\
             <rdf:Description rdf:about=\"\" xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\"\n\
             \x20 tiff:ImageWidth=\"9504\">\n\
             \x20<tiff:ImageLength>6336</tiff:ImageLength>\n\
             \x20<tiff:Orientation>8</tiff:Orientation>\n\
             </rdf:Description>\n\
             </rdf:RDF></x:xmpmeta>";
    let frame = FrameAspect::from_xmp(doc).expect("the mixed-spelling element declares both");
    assert_eq!(
        frame,
        FrameAspect::from_size_turned(9504.0, 6336.0, rawler::Orientation::from_u16(8))
            .expect("a positive rectangle"),
        "all three come from the ONE element that declares the pair, either spelling"
    );
}

/// **C** — a bare fragment with no `rdf:Description` at all.
///
/// Both shapes fall back to the whole text, and that is load-bearing: this
/// module's own fixtures hand `from_xmp` snippets, and a reader given a
/// fragment cannot be mixing two elements' declarations because there are
/// no elements to mix. The element-form fragment is the sharper of the two
/// — it HAS markup, so it pins that the fallback keys on the absence of an
/// `rdf:Description`, not on the absence of tags.
///
/// MUTATION: make `resolve` return an empty span when no Description
/// matched and both halves of this test lose their frame.
#[test]
fn the_frame_scope_falls_back_to_a_bare_fragment() {
    let attrs = "tiff:ImageWidth=\"800\" tiff:ImageLength=\"600\" tiff:Orientation=\"1\"";
    let frame = FrameAspect::from_xmp(attrs).expect("an attribute fragment declares a frame");
    assert_eq!((frame.w, frame.h), (800.0, 600.0));
    let elems = "<tiff:ImageWidth>800</tiff:ImageWidth>\n\
             <tiff:ImageLength>600</tiff:ImageLength>";
    let frame = FrameAspect::from_xmp(elems).expect("an element fragment declares a frame");
    assert_eq!(
        (frame.w, frame.h),
        (800.0, 600.0),
        "markup without an rdf:Description is still a fragment"
    );
}

/// **D** — `rdf:Description`s present, none of them carrying both
/// dimensions.
///
/// Three elements, three separate properties: an orientation, a width, a
/// length. `resolve` finds no complete pair, falls back to the whole
/// document, and the frame is assembled from all THREE — the exact reading
/// F4 symptom D indicted, kept because refusing would drop the frame for a
/// document class nobody has measured (the aspect is disclosed as degraded
/// downstream either way). This is the residue R29-2 names rather than
/// closes.
///
/// MUTATION: return the FIRST `rdf:Description` seen instead of the
/// whole-document fallback and the frame disappears — that element declares
/// only an orientation.
#[test]
fn descriptions_without_a_complete_pair_read_as_the_whole_document() {
    let doc = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
             <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
             <rdf:Description rdf:about=\"\" xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\"\n\
             \x20 tiff:Orientation=\"8\"/>\n\
             <rdf:Description rdf:about=\"\" tiff:ImageWidth=\"9504\"/>\n\
             <rdf:Description rdf:about=\"\" tiff:ImageLength=\"6336\"/>\n\
             </rdf:RDF></x:xmpmeta>";
    let frame = FrameAspect::from_xmp(doc).expect("the whole-document fallback still reads");
    assert_eq!(
        frame,
        FrameAspect::from_size_turned(9504.0, 6336.0, rawler::Orientation::from_u16(8))
            .expect("a positive rectangle"),
        "no element declares the pair, so all three properties come from the document"
    );
}

// Authored synthetic MaskBrushTable payloads, Brotli-compressed once with
// Python's `brotli` module. No byte below comes from a user specimen.
const MB_GOOD_A_LEN: usize = 185;
const MB_GOOD_A: &[u8] = &[
    0x1B, 0xB8, 0x00, 0xF8, 0x8F, 0xC2, 0xB6, 0xB5, 0x73, 0x94, 0x79, 0x28, 0xD3,
    0x42, 0xF8, 0xC9, 0x20, 0x88, 0x9B, 0xDF, 0xC6, 0x02, 0xEA, 0x3A, 0x0F, 0x6C,
    0x2C, 0x91, 0x28, 0x0E, 0x3C, 0xF0, 0x31, 0x51, 0xD6, 0x46, 0xAC, 0x01, 0x14,
    0x4E, 0xC4, 0xC3, 0x06, 0x9C, 0xA8, 0x07, 0x1E, 0xA0, 0x47, 0x32, 0xDD, 0x01,
    0x20, 0x10, 0xC7, 0x27, 0x96, 0x08, 0x80, 0x08, 0x00, 0x08, 0x00, 0x00, 0x58,
    0x10, 0xF9, 0xEE, 0xDC, 0x49, 0x6B, 0xC2, 0x58, 0x07, 0x20, 0x02, 0x42, 0x78,
    0x81, 0x98, 0x81, 0x5A, 0xD8, 0xAA, 0xBC, 0x89, 0xFA, 0x9B, 0xAA, 0x71, 0x28,
    0x13, 0x13, 0xC2, 0x58, 0xB7, 0xC6, 0x30, 0x36, 0x54, 0xBF, 0x44, 0x93, 0x3B,
    0x1A,
];
const MB_GOOD_B_LEN: usize = 87;
const MB_GOOD_B: &[u8] = &[
    0x1B, 0x56, 0x00, 0xF8, 0x9F, 0x07, 0x76, 0x0C, 0x99, 0x22, 0x68, 0xF8, 0x02,
    0xE9, 0xA5, 0x10, 0x26, 0xF7, 0x24, 0xE1, 0x08, 0xDB, 0x12, 0x4C, 0x23, 0xA8,
    0x84, 0xA0, 0x93, 0xE0, 0x81, 0xBA, 0x12, 0x66, 0x61, 0x03, 0x4E, 0x38, 0x0D,
    0x14, 0x47, 0x5A, 0x66, 0xBF, 0x1C, 0x20, 0xC1, 0x40, 0x03, 0xB5, 0x8C, 0xFB,
    0xE9, 0xD1, 0x02, 0x81, 0xBC, 0x35, 0x01,
];
const MB_UNKNOWN_OPCODE_LEN: usize = 83;
const MB_UNKNOWN_OPCODE: &[u8] = &[
    0x1B, 0x52, 0x00, 0xF8, 0x07, 0x61, 0x73, 0x13, 0xE9, 0x1A, 0xA2, 0xCD, 0x52,
    0xE5, 0xBC, 0x45, 0xD0, 0x05, 0x99, 0x7A, 0xA4, 0x03, 0x01, 0x80, 0x40, 0xA0,
    0x3C, 0x0C, 0x3E, 0xDD, 0x2B, 0xBD, 0x64, 0x08, 0xE4,
];
const MB_TRAILING_LEN: usize = 88;
const MB_TRAILING: &[u8] = &[
    0x1B, 0x57, 0x00, 0xF8, 0x9F, 0x07, 0x76, 0x0C, 0x99, 0x22, 0x68, 0xF8, 0x02,
    0xE9, 0xA5, 0x10, 0x26, 0xF7, 0x24, 0xE1, 0x08, 0xDB, 0x12, 0x4C, 0x23, 0xA8,
    0x84, 0xA0, 0x93, 0xE0, 0x81, 0xBA, 0x12, 0x66, 0x61, 0x03, 0x4E, 0x38, 0x0D,
    0x18, 0x37, 0x5A, 0x66, 0xBF, 0x1C, 0x20, 0xC1, 0x40, 0x03, 0xB5, 0x8C, 0xFB,
    0xE9, 0xD1, 0x02, 0x81, 0xBC, 0x35, 0x01,
];
const MB_TABLE_WORD_LEN: usize = 8;
const MB_TABLE_WORD: &[u8] =
    &[0x1B, 0x07, 0x00, 0xF8, 0xA7, 0x00, 0x04, 0x82, 0x92, 0x40, 0x20];
const MB_RECORD_BOUND_LEN: usize = 8;
const MB_RECORD_BOUND: &[u8] =
    &[0x8B, 0x03, 0x80, 0x01, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x03];
const MB_DCOUNT_BOUND_LEN: usize = 78;
const MB_DCOUNT_BOUND: &[u8] = &[
    0x1B, 0x4D, 0x00, 0xF8, 0x07, 0xE1, 0x64, 0x17, 0x12, 0x21, 0x6A, 0x4A, 0x35,
    0x1B, 0xD8, 0x80, 0x13, 0x4E, 0x03, 0x87, 0x05, 0x06, 0x5A, 0x4E, 0x07, 0x02,
    0x68, 0xC9, 0xC0, 0x02, 0xFD, 0xBC, 0xDA, 0x4B, 0x35, 0x14, 0x78, 0xAF,
];
const MB_TOKEN_BOUND_LEN: usize = 327_772;
const MB_TOKEN_BOUND: &[u8] = &[
    0x5B, 0x5B, 0x00, 0x85, 0x7F, 0x28, 0xF0, 0x76, 0x5F, 0x54, 0x42, 0xD4, 0x94,
    0x6A, 0x82, 0x76, 0x44, 0x97, 0xAE, 0x7E, 0x93, 0x08, 0x5C, 0xC0, 0x55, 0x49,
    0x08, 0x10, 0x80, 0x8D, 0x4F, 0xF7, 0x4D, 0xEB, 0xD0, 0x7D, 0xEF, 0x09, 0x20,
    0x84, 0xB1, 0x1F, 0x08,
];

fn mb_object(stream: &[u8]) -> Vec<u8> {
    let mut object = Vec::with_capacity(16 + stream.len());
    for word in [4u32, 1, 64_000, stream.len() as u32] {
        object.extend_from_slice(&word.to_le_bytes());
    }
    object.extend_from_slice(stream);
    object
}

fn mb_acr(objects: &[Vec<u8>]) -> (Vec<u8>, Vec<String>) {
    let directory_end = 20 + 32 * objects.len();
    let mut offsets = Vec::with_capacity(objects.len());
    let mut at = directory_end as u64;
    for object in objects {
        offsets.push(at);
        at += object.len() as u64;
        at += (4 - at % 4) % 4;
    }
    let mut acr = Vec::with_capacity(at as usize);
    acr.extend_from_slice(b"ACR\0");
    acr.extend_from_slice(&1u32.to_le_bytes());
    acr.extend_from_slice(b"ARW\0");
    acr.extend_from_slice(&(objects.len() as u32).to_le_bytes());
    acr.extend_from_slice(&0u32.to_le_bytes());
    let mut tokens = Vec::new();
    for (object, offset) in objects.iter().zip(offsets) {
        let digest = md5::compute(object);
        acr.extend_from_slice(&digest.0);
        acr.extend_from_slice(&(object.len() as u64).to_le_bytes());
        acr.extend_from_slice(&offset.to_le_bytes());
        tokens.push(format!("{digest:X}"));
    }
    for object in objects {
        acr.extend_from_slice(object);
        while acr.len() % 4 != 0 {
            acr.push(0);
        }
    }
    (acr, tokens)
}

fn mb_temp(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "autoshade-mask-brush-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let raw = dir.join("synthetic.arw");
    std::fs::write(&raw, b"synthetic raw identity").unwrap();
    (dir, raw)
}

fn mb_group(name: &str, token: &str, bytes: usize) -> String {
    format!(
        "<rdf:li crs:What=\"Mask/Aggregate\" crs:MaskActive=\"true\" \
             crs:MaskName=\"{name}\" crs:MaskBlendMode=\"0\" crs:MaskInverted=\"false\" \
             crs:MaskSyncID=\"0000000000000000000000000000000D\" crs:MaskValue=\"1\" \
             crs:MaskBrushTable=\"{token}\" crs:MaskBrushUncompressedBytes=\"{bytes}\"/>\n"
    )
}

fn mb_doc(groups: &[(&str, &str, usize)]) -> String {
    let corrections: String = groups
        .iter()
        .map(|(correction, token, bytes)| {
            lr_correction(correction, "", &mb_group("Brush 1", token, *bytes))
        })
        .collect();
    lr_doc(&corrections)
}

fn mb_parse(
    raw: &std::path::Path,
    doc: &str,
) -> (EditRecipe, Vec<crate::diag::Line>) {
    let collector = crate::diag::Collector::new();
    let diag = crate::diag::Diag::about(&collector, raw);
    let recipe = xmp_to_recipe_with_diag(doc, &diag);
    (recipe, collector.take())
}

fn mb_assert_refusal(
    tag: &str,
    acr: Option<&[u8]>,
    token: &str,
    advertised: usize,
    expected: MaskBrushTableRefusal,
) {
    let (dir, raw) = mb_temp(tag);
    if let Some(acr) = acr {
        std::fs::write(raw.with_extension("acr"), acr).unwrap();
    }
    let doc = mb_doc(&[("Mask 1", token, advertised)]);
    let (recipe, lines) = mb_parse(&raw, &doc);
    assert!(recipe.masks.is_empty(), "a refused table imported partial geometry");
    let matching: Vec<_> = lines
        .iter()
        .filter(|line| line.text.contains(expected.name()))
        .collect();
    assert_eq!(matching.len(), 1, "named refusal must be loud exactly once: {lines:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn mask_brush_tables_import_independently_in_owner_and_table_order() {
    let (dir, raw) = mb_temp("happy-multi");
    let objects = [mb_object(MB_GOOD_A), mb_object(MB_GOOD_B)];
    let (acr, tokens) = mb_acr(&objects);
    std::fs::write(raw.with_extension("acr"), acr).unwrap();
    let doc = mb_doc(&[
        ("First", &tokens[0], MB_GOOD_A_LEN),
        ("Second", &tokens[1], MB_GOOD_B_LEN),
    ]);
    let (recipe, lines) = mb_parse(&raw, &doc);
    assert!(lines.is_empty(), "valid tables emitted diagnostics: {lines:?}");
    assert_eq!(recipe.masks.len(), 2);
    let MaskGeometry::Brush { strokes: first, .. } = &recipe.masks[0].mask else {
        panic!("first table did not stay with its aggregate")
    };
    let MaskGeometry::Brush { strokes: second, .. } = &recipe.masks[1].mask else {
        panic!("second table did not stay with its aggregate")
    };
    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 1);
    assert_eq!(first[0].sync_id, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(first[1].sync_id, "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB");
    assert_eq!(second[0].sync_id, "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn mask_brush_fixed_point_fields_and_r_f_d_tokens_map_exactly() {
    let (dir, raw) = mb_temp("fixed-point");
    let (acr, tokens) = mb_acr(&[mb_object(MB_GOOD_A)]);
    std::fs::write(raw.with_extension("acr"), acr).unwrap();
    let recipe = mb_parse(&raw, &mb_doc(&[("Mask 1", &tokens[0], MB_GOOD_A_LEN)])).0;
    let MaskGeometry::Brush { strokes, .. } = &recipe.masks[0].mask else { panic!() };
    assert_eq!((strokes[0].value * 1_000_000.0).round() as u32, 51_402);
    assert_eq!((strokes[0].radius * 1_000_000.0).round() as u32, 36_957);
    assert_eq!((strokes[0].flow * 1_000_000.0).round() as u32, 1_000_000);
    assert_eq!(strokes[0].center_weight, 0.0);
    assert_eq!(
        strokes[0].dabs,
        "r 0.123456\nf 0.0103\nd 0.404621 0.692602\nd 0.401151 0.693698"
    );
    assert!(!strokes[0].dabs.lines().any(|token| token.starts_with("h ")));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn missing_mask_brush_companion_is_mask_brush_table_unavailable() {
    mb_assert_refusal(
        "unavailable",
        None,
        "00000000000000000000000000000000",
        MB_GOOD_A_LEN,
        MaskBrushTableRefusal::MaskBrushTableUnavailable,
    );
}

#[test]
fn malformed_mask_brush_directory_is_container_invalid() {
    let (mut acr, tokens) = mb_acr(&[mb_object(MB_GOOD_A)]);
    acr[12..16].copy_from_slice(&((MAX_ACR_DIRECTORY_ENTRIES + 1) as u32).to_le_bytes());
    mb_assert_refusal(
        "container",
        Some(&acr),
        &tokens[0],
        MB_GOOD_A_LEN,
        MaskBrushTableRefusal::ContainerInvalid,
    );

    let (mut acr, tokens) = mb_acr(&[mb_object(MB_GOOD_A), mb_object(MB_GOOD_B)]);
    let len = le_u64_at(&acr, 36);
    let offset = le_u64_at(&acr, 44);
    let padding = usize::try_from(offset + len).unwrap();
    assert_eq!(acr[padding], 0, "fixture must have inter-object padding");
    acr[padding] = 1;
    mb_assert_refusal(
        "container-padding",
        Some(&acr),
        &tokens[0],
        MB_GOOD_A_LEN,
        MaskBrushTableRefusal::ContainerInvalid,
    );
}

#[test]
fn absent_mask_brush_key_is_reference_mismatch() {
    let (acr, _) = mb_acr(&[mb_object(MB_GOOD_A)]);
    mb_assert_refusal(
        "reference",
        Some(&acr),
        "00000000000000000000000000000000",
        MB_GOOD_A_LEN,
        MaskBrushTableRefusal::ReferenceMismatch,
    );
}

#[test]
fn changed_mask_brush_blob_is_digest_mismatch() {
    let (mut acr, tokens) = mb_acr(&[mb_object(MB_GOOD_A)]);
    let len = le_u64_at(&acr, 36);
    let offset = le_u64_at(&acr, 44);
    let last = usize::try_from(offset + len - 1).unwrap();
    acr[last] ^= 0x01;
    mb_assert_refusal(
        "digest",
        Some(&acr),
        &tokens[0],
        MB_GOOD_A_LEN,
        MaskBrushTableRefusal::DigestMismatch,
    );
}

#[test]
fn unknown_mask_brush_envelope_is_encoding_unsupported() {
    let mut object = mb_object(MB_GOOD_A);
    object[0..4].copy_from_slice(&5u32.to_le_bytes());
    let (acr, tokens) = mb_acr(&[object]);
    mb_assert_refusal(
        "encoding",
        Some(&acr),
        &tokens[0],
        MB_GOOD_A_LEN,
        MaskBrushTableRefusal::EncodingUnsupported,
    );
}

#[test]
fn invalid_mask_brush_brotli_is_corrupt() {
    let (acr, tokens) = mb_acr(&[mb_object(&[0xFF])]);
    mb_assert_refusal(
        "corrupt",
        Some(&acr),
        &tokens[0],
        1,
        MaskBrushTableRefusal::Corrupt,
    );
}

#[test]
fn wrong_mask_brush_advertised_size_is_length_mismatch() {
    let (acr, tokens) = mb_acr(&[mb_object(MB_GOOD_A)]);
    mb_assert_refusal(
        "length",
        Some(&acr),
        &tokens[0],
        MB_GOOD_A_LEN + 1,
        MaskBrushTableRefusal::LengthMismatch,
    );
}

#[test]
fn binary_h_opcode_is_payload_unsupported() {
    let (acr, tokens) = mb_acr(&[mb_object(MB_UNKNOWN_OPCODE)]);
    mb_assert_refusal(
        "payload-unsupported",
        Some(&acr),
        &tokens[0],
        MB_UNKNOWN_OPCODE_LEN,
        MaskBrushTableRefusal::PayloadUnsupported,
    );
}

#[test]
fn trailing_mask_brush_payload_is_payload_invalid() {
    let (acr, tokens) = mb_acr(&[mb_object(MB_TRAILING)]);
    mb_assert_refusal(
        "payload-invalid",
        Some(&acr),
        &tokens[0],
        MB_TRAILING_LEN,
        MaskBrushTableRefusal::PayloadInvalid,
    );
}

#[test]
fn one_bad_mask_brush_table_does_not_take_down_an_independent_table() {
    let objects = [mb_object(MB_GOOD_B), mb_object(MB_UNKNOWN_OPCODE)];
    let (acr, tokens) = mb_acr(&objects);
    let (dir, raw) = mb_temp("independent-refusal");
    std::fs::write(raw.with_extension("acr"), acr).unwrap();
    let survivor = lr_paint(
        "1111111111111111111111111111111C",
        "1",
        "0",
        "false",
        &["d 0.25 0.75"],
    );
    let bad_group = format!(
        "<rdf:li crs:What=\"Mask/Aggregate\" crs:MaskActive=\"true\" \
             crs:MaskName=\"Bad table\" crs:MaskBlendMode=\"0\" crs:MaskInverted=\"false\" \
             crs:MaskSyncID=\"0000000000000000000000000000000E\" crs:MaskValue=\"1\" \
             crs:MaskBrushTable=\"{}\" crs:MaskBrushUncompressedBytes=\"{}\">\n{}\
             </rdf:li>\n",
        tokens[1], MB_UNKNOWN_OPCODE_LEN, survivor
    );
    let doc = lr_doc(&format!(
        "{}{}",
        lr_correction(
            "Good",
            "",
            &mb_group("Brush 1", &tokens[0], MB_GOOD_B_LEN),
        ),
        lr_correction("Bad", "", &bad_group),
    ));
    let (recipe, lines) = mb_parse(&raw, &doc);
    assert_eq!(
        recipe.masks.len(),
        2,
        "the good table and bad table's text survivor must both import: {recipe:?}"
    );
    let MaskGeometry::Brush { strokes, .. } = &recipe.masks[1].mask else { panic!() };
    assert_eq!(strokes.len(), 1, "a refused table must contribute no partial records");
    assert_eq!(strokes[0].sync_id, "1111111111111111111111111111111C");
    assert!(lines.iter().any(|line| line.text.contains("PayloadUnsupported")));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn non_one_mask_brush_table_word_is_payload_unsupported() {
    let (acr, tokens) = mb_acr(&[mb_object(MB_TABLE_WORD)]);
    mb_assert_refusal(
        "table-word",
        Some(&acr),
        &tokens[0],
        MB_TABLE_WORD_LEN,
        MaskBrushTableRefusal::PayloadUnsupported,
    );
}

#[test]
fn mask_brush_record_count_bound_refuses_before_allocation() {
    let (acr, tokens) = mb_acr(&[mb_object(MB_RECORD_BOUND)]);
    mb_assert_refusal(
        "record-bound",
        Some(&acr),
        &tokens[0],
        MB_RECORD_BOUND_LEN,
        MaskBrushTableRefusal::PayloadInvalid,
    );
}

#[test]
fn mask_brush_d_count_bound_refuses_before_token_walk() {
    let (acr, tokens) = mb_acr(&[mb_object(MB_DCOUNT_BOUND)]);
    mb_assert_refusal(
        "d-count-bound",
        Some(&acr),
        &tokens[0],
        MB_DCOUNT_BOUND_LEN,
        MaskBrushTableRefusal::PayloadInvalid,
    );
}

#[test]
fn mask_brush_token_count_bound_covers_unbounded_state_tokens() {
    let (acr, tokens) = mb_acr(&[mb_object(MB_TOKEN_BOUND)]);
    mb_assert_refusal(
        "token-bound",
        Some(&acr),
        &tokens[0],
        MB_TOKEN_BOUND_LEN,
        MaskBrushTableRefusal::PayloadInvalid,
    );
}

#[test]
fn table_import_preserves_residual_aggregate_and_gesture_paints() {
    let (dir, raw) = mb_temp("survivors");
    let (acr, tokens) = mb_acr(&[mb_object(MB_GOOD_B)]);
    std::fs::write(raw.with_extension("acr"), acr).unwrap();
    let aggregate_survivor = lr_brush_group("false", "");
    let gesture = lr_paint(
        "1111111111111111111111111111111B",
        "1",
        "0",
        "false",
        &["h 1.0000", "d 0.25 0.75"],
    )
    .replace(
        "crs:CenterWeight=\"0\">",
        "crs:CenterWeight=\"0\" crs:BrushGestureInterpretation=\"0\">",
    );
    let corrections = format!(
        "{}{}",
        lr_correction(
            "Brushes",
            "",
            &format!(
                "{}{}",
                mb_group("Binary", &tokens[0], MB_GOOD_B_LEN),
                aggregate_survivor
            ),
        ),
        lr_correction("Gesture", "", &lr_ai_mask("0", "0", "1", "", &gesture)),
    );
    let doc = lr_doc(&corrections);
    let recipe = mb_parse(&raw, &doc).0;
    assert_eq!(recipe.masks.len(), 2);
    assert_eq!(recipe.masks[0].components.len(), 1, "aggregate survivor was dropped");
    let MaskGeometry::AiMask { gesture, .. } = &recipe.masks[1].mask else { panic!() };
    assert_eq!(gesture.len(), 1, "gesture survivor was dropped");
    assert!(gesture[0].dabs.starts_with("h 1.0000\n"), "text h token must survive");
    let merged = merge_recipe_into_xmp_in_frame_for_photo(&doc, &recipe, None, Some(&raw))
        .expect("the survivor document merges");
    assert!(merged.doc.contains("crs:BrushGestureInterpretation=\"0\""));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn unchanged_table_mask_round_trip_keeps_attributes_without_text_paints() {
    let (dir, raw) = mb_temp("writer-round-trip");
    let (acr, tokens) = mb_acr(&[mb_object(MB_GOOD_A)]);
    std::fs::write(raw.with_extension("acr"), acr).unwrap();
    let doc = mb_doc(&[("Mask 1", &tokens[0], MB_GOOD_A_LEN)]);
    let mut recipe = mb_parse(&raw, &doc).0;
    recipe.exposure_ev = 0.75;
    let merged = merge_recipe_into_xmp_in_frame_for_photo(&doc, &recipe, None, Some(&raw))
        .expect("table-bearing base must merge");
    assert_eq!(merged.doc.matches("crs:MaskBrushTable=").count(), 1);
    assert!(merged.doc.contains(&format!("crs:MaskBrushTable=\"{}\"", tokens[0])));
    assert!(merged.doc.contains(&format!(
        "crs:MaskBrushUncompressedBytes=\"{MB_GOOD_A_LEN}\""
    )));
    assert_eq!(
        merged.doc.matches("crs:What=\"Mask/Paint\"").count(),
        0,
        "table records must not be synthesized as duplicate text Paints"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn env_mask_brush_sample_matches_stage_three_ground_truth() {
    let Some(root) = crate::config::live_env("AUTOSHADE_MB_SAMPLE_ROOT") else {
        crate::test_skipped(
            "env_mask_brush_sample_matches_stage_three_ground_truth",
            "AUTOSHADE_MB_SAMPLE_ROOT unset",
        );
        return;
    };
    let root = std::path::Path::new(&root);
    let xmp_path = root.join("P04-rewritten-brushtable.xmp");
    let photo = root.join("P04-rewritten.arw");
    let text = std::fs::read_to_string(&xmp_path).expect("read rewritten P04 XMP");
    let collector = crate::diag::Collector::new();
    let diag = crate::diag::Diag::about(&collector, &photo);
    let recipe = xmp_to_recipe_with_diag(&text, &diag);
    assert!(collector.take().is_empty(), "the confirmed specimen must parse without refusal");
    let mut tables: Vec<&Vec<BrushStroke>> = Vec::new();
    for mask in &recipe.masks {
        for geometry in std::iter::once(&mask.mask)
            .chain(mask.components.iter().map(|component| &component.geometry))
        {
            if let MaskGeometry::Brush { strokes, .. } = geometry
                && matches!(strokes.len(), 54 | 4 | 18)
            {
                tables.push(strokes);
            }
        }
    }
    assert_eq!(
        tables.iter().map(|table| table.len()).collect::<Vec<_>>(),
        [54, 4, 18],
        "the three table record counts stay in XMP order"
    );
    let d_count: usize = tables
        .iter()
        .flat_map(|table| table.iter())
        .map(|stroke| stroke.dabs.lines().filter(|token| token.starts_with("d ")).count())
        .sum();
    assert_eq!(d_count, 3_043);
    let t2_first = &tables[1][0];
    assert_eq!((t2_first.value * 1_000_000.0).round() as u32, 51_402);
    assert_eq!(t2_first.dabs.lines().next(), Some("d 0.404621 0.692602"));
}
