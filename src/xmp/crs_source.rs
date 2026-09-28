//! The typed crs source: Tag and Scope as CrsSource, the unparsable-number report, XML unescaping, and the curve and point-colour parsers.

use super::*;

/// **The scope a `crs:` read is allowed to see — as a TYPE** (R28 Batch-5 5d,
/// adjudication F4 root 1).
///
/// Every reader below used to take a bare `&str`, and that string carried TWO
/// incompatible meanings depending on which call site produced it: "one
/// element's start tag" (read its attributes) or "a subtree" (first match
/// anywhere inside, children included). The distinction was a per-call-site
/// CONVENTION — nothing in a signature said which was meant, nothing checked,
/// and the difference is exactly the class of defect R27 Batch-4 hardened three
/// sites against by hand (`components_in`, `correction_mask_components`,
/// `base_element`) while `xmp.rs`'s own comment admitted "the older reads stay
/// on `g`" was a survey, not an invariant.
///
/// So the scope is a type now. [`Tag`] can only ever answer from ONE element's
/// attributes; [`Scope`] is the subtree search, and a call site that wants one
/// cannot silently get the other — it has to name it.
pub(crate) trait CrsSource<'a>: Copy {
    /// Raw string value of `crs:<key>` within this source's scope. The `crs:`
    /// anchor makes prefixed cousins unambiguous (`crs:Tint` can never match
    /// inside `crs:LocalTint`).
    fn crs_str(self, key: &str) -> Option<std::borrow::Cow<'a, str>>;

    /// Numeric `crs:` value, tolerating ACR's explicit `+` (`"+22"`). `None`
    /// if the key is absent, unparsable, or NON-FINITE: Rust's f32 parser
    /// accepts "NaN"/"inf", no real sidecar writer emits them, and letting one
    /// through imported a value the recipe clamp then silently neutralised
    /// WITHOUT the unparsable-number disclosure ever firing.
    fn crs_f32(self, key: &str) -> Option<f32> {
        self.crs_str(key)?
            .trim()
            .trim_start_matches('+')
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite())
    }
}

/// ONE element's own start tag, `<` to `>` inclusive (or a bare attribute list).
///
/// A read against a `Tag` sees that element's ATTRIBUTES and nothing else: no
/// body, no children, no siblings. It is the right scope for every per-component
/// question — "what does THIS `<rdf:li crs:What="Mask/…">` say" — and it is not
/// expressible as a subtree read, which is the point.
#[derive(Clone, Copy)]
pub(crate) struct Tag<'a>(&'a str);

/// A SUBTREE: an element and everything nested inside it, or a whole document.
///
/// A read against a `Scope` takes the FIRST match anywhere inside, children
/// included — which is what a whole-document read of `crs:Exposure2012` needs
/// (the property may be an attribute on the Description or a child element of
/// it) and what a per-element read must never get. When the enclosing element
/// has nested settings that are somebody ELSE's, the scope is narrowed BEFORE
/// it is built: [`crs_own_scope`] does that for the top-level Description and
/// [`correction_own_scope`] for one correction.
#[derive(Clone, Copy)]
pub(crate) struct Scope<'a>(&'a str);

impl<'a> Tag<'a> {
    pub(crate) fn new(tag: &'a str) -> Self {
        Tag(tag)
    }

    /// The underlying text, for the structural helpers (`tag_name`,
    /// `next_xml_attribute`) that work on markup rather than on one property.
    pub(super) fn text(self) -> &'a str {
        self.0
    }
}

impl<'a> Scope<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Scope(text)
    }

    /// The underlying text, for the structural helpers (`owned_element_body`,
    /// `parse_curve_checked`, `next_xml_tag`) that walk markup rather than read
    /// one property. `pub(crate)`: `eval`'s tone-curve reader is one of those
    /// helpers and lives in another module.
    pub(crate) fn text(self) -> &'a str {
        self.0
    }

    /// Byte offset of the first tag inside this scope carrying
    /// `crs:<key>="<wanted>"`. Subtree-wide BY DEFINITION — the callers are
    /// "does this block hold a correction" and "where is the component list's
    /// range mask", both of which are questions about a region.
    pub(super) fn find_value_at(self, key: &str, wanted: &str) -> Option<usize> {
        let xmp = self.0;
        if xmp.len() > MAX_XMP_BYTES {
            return None;
        }
        let name = format!("crs:{key}");
        if !xmp.trim_start().starts_with('<')
            && let Some((_, raw)) = xml_attribute_raw(xmp, &name)
            && xml_unescape(raw).as_ref() == wanted
        {
            return Some(0);
        }

        let mut from = 0;
        while let Some((start, end, _)) = next_xml_tag(xmp, from) {
            let tag = &xmp[start..=end];
            if !tag.starts_with("</")
                && let Some((_, raw)) = xml_attribute_raw(tag, &name)
                && xml_unescape(raw).as_ref() == wanted
            {
                return Some(start);
            }
            from = end + 1;
        }
        None
    }
}

impl<'a> CrsSource<'a> for Tag<'a> {
    /// Attributes only. No tag walk, no element-body form — a start tag HAS no
    /// body, and a `Tag` built from something larger still cannot read past the
    /// first element's attributes, because that is all `xml_attribute_raw`
    /// looks at (`next_xml_attribute` stops at the tag's own `/` or `>`).
    fn crs_str(self, key: &str) -> Option<std::borrow::Cow<'a, str>> {
        if self.0.len() > MAX_XMP_BYTES {
            return None;
        }
        let name = format!("crs:{key}");
        xml_attribute_raw(self.0, &name).map(|(_, raw)| xml_unescape(raw))
    }
}

impl<'a> CrsSource<'a> for Scope<'a> {
    /// First occurrence anywhere in the subtree, in either XMP spelling: an
    /// attribute on any tag, or a `<crs:Key>…</crs:Key>` property element.
    fn crs_str(self, key: &str) -> Option<std::borrow::Cow<'a, str>> {
        let xmp = self.0;
        if xmp.len() > MAX_XMP_BYTES {
            return None;
        }
        let name = format!("crs:{key}");
        if !xmp.trim_start().starts_with('<')
            && let Some((_, raw)) = xml_attribute_raw(xmp, &name)
        {
            return Some(xml_unescape(raw));
        }

        let mut from = 0;
        while let Some((start, end, self_closing)) = next_xml_tag(xmp, from) {
            let tag = &xmp[start..=end];
            if !tag.starts_with("</") {
                if let Some((_, raw)) = xml_attribute_raw(tag, &name) {
                    return Some(xml_unescape(raw));
                }
                if tag_name(tag) == name {
                    if self_closing {
                        return Some(std::borrow::Cow::Borrowed(""));
                    }
                    // By NAME, not the literal `</crs:Key>`: `</crs:Key >` is
                    // the same close in XML, and the literal ran past it into
                    // the next occurrence (or off the document).
                    let close_at = element_close_start(xmp, &name, end)?;
                    return Some(xml_unescape(xmp[end + 1..close_at].trim()));
                }
            }
            from = end + 1;
        }
        None
    }
}



/// Owned crs settings PRESENT in a document whose value does not parse as a
/// number under [`crs_f32`]'s exact rule. Each of these imports as a SILENT
/// neutral in [`xmp_to_recipe`], and the next save then overwrites the
/// sidecar with those neutrals — so restore surfaces disclose them (GUI open
/// note, web X-Recipe-Warning, store derived-snapshot trace). String-typed
/// owned keys are exempt.
pub fn unparsable_crs_numbers(xmp: &str) -> Vec<String> {
    const STRINGY: [&str; 8] = [
        "Version",
        "ProcessVersion",
        "WhiteBalance",
        "HasCrop",
        "ToneCurveName2012",
        "HasSettings",
        // A FLAG, not a number (R25 B3). Lightroom writes 0/1, but "true" is
        // the other spelling a crs boolean takes in the wild and the reader
        // accepts both — naming it here as unparsable would be a disclosure
        // about a value that imported perfectly.
        "AutoLateralCA",
        // The B&W switch (v1.5.0), Lightroom's `"True"` / `"False"`.
        "ConvertToGrayscale",
    ];
    if xmp.len() > MAX_XMP_BYTES {
        return vec!["XMP document exceeds the 16 MiB limit".to_string()];
    }
    // A foreign namespace binding means every scanner below is reading the
    // wrong (or no) property — one entry naming the binding beats a silent
    // fully-neutral import. Reaches the GUI open note, the web
    // X-Recipe-Warning and the store trace through the existing plumbing.
    if let Some(conflict) = xmlns_conflict(xmp) {
        return vec![format!("{conflict} — its camera-raw settings were not imported")];
    }

    let scope = crs_own_scope(xmp);
    // ONE `Scope` for every read below: the Description's OWN span, which is
    // what this whole-document scan has always meant (R28 Batch-5 5d makes it
    // say so in the type instead of by convention).
    let scope = Scope::new(scope.as_ref());
    let mut bad: Vec<String> = owned_attr_keys()
        .into_iter()
        .filter(|k| !STRINGY.contains(&k.as_str()))
        // The PASS-THROUGH nine are EXEMPT, and not as a special case — as
        // the definition of the tier. This scan exists because an owned key
        // whose value does not parse "imports as a SILENT neutral, and the
        // next save overwrites the sidecar with those neutrals". A
        // pass-through property has no neutral to import as: it is never read
        // as a number, never clamped and never replaced — it goes back out as
        // the same string, in range or out, numeric or not. Naming
        // `crs:CameraProfile="Adobe Standard"` here would be a warning about a
        // value that round-tripped perfectly (R25 B4); so would
        // `crs:PerspectiveX="-140"`, which is out of the ±100 default band
        // this scan falls back to and is a perfectly ordinary Upright result.
        .filter(|k| !PASSTHROUGH_CRS.contains(&k.as_str()))
        // …and neither is an OWNED key whose registry shape is not a number
        // (v1.5.0 F7). `crs:CameraProfile` left the carried list and joined the
        // owned one, and this scan's universe is the owned list — so the same
        // profile name that was correctly exempt as a carried key came back as
        // an "unparsable number" the moment it became a control. Asking the
        // registry for the SHAPE closes the class rather than adding one more
        // name to the list above: a future text or boolean control is exempt
        // the day it is registered.
        .filter(|k| {
            !crate::advisor::catalogue::RECIPE_CONTROLS.iter().any(|c| {
                c.crs.attr() == Some(k.as_str())
                    && !matches!(
                        c.shape,
                        crate::advisor::catalogue::Shape::Number
                            | crate::advisor::catalogue::Shape::NullableNumber
                            | crate::advisor::catalogue::Shape::Integer
                    )
            })
        })
        .filter(|k| {
            scope.crs_str(k).is_some()
                && scope
                    .crs_f32(k)
                    .is_none_or(|v| !crs_number_is_in_recipe_range(k, v))
        })
        .collect();
    for tag in [
        "ToneCurvePV2012",
        "ToneCurvePV2012Red",
        "ToneCurvePV2012Green",
        "ToneCurvePV2012Blue",
    ] {
        if parse_curve_checked(scope.text(), tag).is_err() {
            bad.push(tag.to_string());
        }
    }
    // The point colours, by the curve rule: a block present but unreadable
    // imports as no swatch at all, and the photo renders without them.
    if parse_point_colors_checked(scope.text()).is_err() {
        bad.push("PointColors".to_string());
    }
    // A structurally inconsistent crop (HasCrop="True" with a missing
    // coordinate, an out-of-domain value, or an ordering the corners cannot
    // have) imports as a SILENT None and the next save persists
    // HasCrop="False" — a deletion nobody asked for. Individually unparsable
    // coordinates are named by the generic scan above; absence and ordering
    // are only visible to a check of the structure as a whole (the curve
    // rule, applied to the crop).
    //
    // The verdict is the READER's own — `read_crop`, the decode
    // `xmp_to_recipe` runs — not a restatement of it. This scan used to
    // require `Left < Right && Top < Bottom` outright, but under the corner
    // encoding `Left > Right` is a legal ROTATED arrangement (see
    // `lr_to_engine_crop`, "no ordering guard") that the reader decodes and
    // renders, so the restated rule disclosed a crop that had imported whole
    // as "inconsistent". Asking the decoder keeps the two in agreement by
    // construction; only its `Refused` arm is the silent None this entry
    // exists for (a frameless tilt nobody can place is `NoFrame`, which
    // `crop_import_note` discloses by name).
    if scope.crs_str("HasCrop").as_deref() == Some("True") {
        let refused = matches!(
            read_crop(scope, FrameAspect::from_xmp(xmp), is_autoshade_sidecar(xmp)),
            CropDecode::Refused { .. }
        );
        if refused && !bad.iter().any(|k| k.starts_with("Crop")) {
            bad.push("Crop (HasCrop=\"True\" with missing or inconsistent coordinates)".to_string());
        }
    }
    bad
}

/// The band a `crs:` number must land in to be a value this app can import —
/// DERIVED from the control registry (`catalogue::RECIPE_CONTROLS`) rather than
/// restated here. A new attribute key used to need an edit in TWO places, this
/// table and [`owned_attr_keys`]; miss this one and the key still imports, but
/// [`unparsable_crs_numbers`] never checks it, so a nonsense value arrives as a
/// silent clamp with no disclosure. Now the writer's list is the only edit.
///
/// Two residues stay spelled out, because the registry states no field for
/// them:
///
///   * **the colour-grade wheels** — one registry row (`color_grade`) stands
///     for 14 attributes, so the per-wheel bands come off the field NAME
///     (`ColorGrade::clamp`: hue 0..360, sat + blending 0..100, lum + balance
///     ±100), keyed through [`COLOR_GRADE_CRS`] so a new wheel inherits them.
///   * **the crop rectangle** — one row for four 0..1 coordinates
///     (`Crop::clamp`); `CropAngle` is a scalar row of its own and derives.
///
/// Everything else falls to ±100 — what `Hsl::clamp` enforces for the 24 mixer
/// attributes and what every remaining signed slider uses.
pub(super) fn crs_number_is_in_recipe_range(key: &str, value: f32) -> bool {
    use crate::advisor::catalogue::{COLOR_GRADE_CRS, RECIPE_CONTROLS};
    let grade_band = |field: &str| {
        if field.ends_with("_hue") {
            (0.0, 360.0)
        } else if field.ends_with("_sat") || field == "blending" {
            (0.0, 100.0)
        } else {
            (-100.0, 100.0)
        }
    };
    let (lo, hi) = if let Some(band) =
        RECIPE_CONTROLS.iter().find(|c| c.crs.attr() == Some(key)).and_then(|c| c.range)
    {
        band
    } else if let Some((field, _)) = COLOR_GRADE_CRS.iter().find(|(_, k)| *k == key) {
        grade_band(field)
    } else if matches!(key, "CropTop" | "CropLeft" | "CropBottom" | "CropRight") {
        (0.0, 1.0)
    } else {
        (-100.0, 100.0)
    };
    (lo..=hi).contains(&value)
}



// `crs_f32` moved onto `CrsSource` in R28 Batch-5 5d — it is the same parse
// applied to whatever `crs_str` answered, and leaving it as a free function
// taking `&str` would have kept the untyped door open beside the typed one.

/// Decode XML character references in one pass. Decoding `&amp;lt;` yields
/// `&lt;`, not `<`, because the logical value must be unescaped exactly once.
pub(super) fn xml_unescape(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains('&') {
        return std::borrow::Cow::Borrowed(s);
    }

    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    while let Some(rel) = s[at..].find('&') {
        let amp = at + rel;
        out.push_str(&s[at..amp]);
        let Some(semi_rel) = s[amp + 1..].find(';') else {
            out.push_str(&s[amp..]);
            return std::borrow::Cow::Owned(out);
        };
        let semi = amp + 1 + semi_rel;
        let entity = &s[amp + 1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => {
                let value = if let Some(hex) =
                    entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X"))
                {
                    u32::from_str_radix(hex, 16).ok()
                } else if let Some(decimal) = entity.strip_prefix('#') {
                    decimal.parse::<u32>().ok()
                } else {
                    None
                };
                value.and_then(char::from_u32).filter(|&c| xml_char_allowed(c))
            }
        };
        if let Some(c) = decoded {
            out.push(c);
        } else {
            out.push_str(&s[amp..=semi]);
        }
        at = semi + 1;
    }
    out.push_str(&s[at..]);
    std::borrow::Cow::Owned(out)
}



/// The text between `open` and `close` (first occurrence of each, in order).
/// For NON-MARKUP text patterns only (the rationale comment scan) — element
/// lookups go through [`owned_element_body`], which matches by tag NAME so
/// attribute-carrying and whitespace-close spellings resolve; a literal
/// element scan here was the reader's silent-loss blind spot (L05#1).
pub(super) fn block_between<'a>(xmp: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = xmp.find(open)? + open.len();
    let rest = &xmp[start..];
    Some(&rest[..rest.find(close)?])
}

/// Parse one `<crs:ToneCurvePV2012…>` `rdf:Seq` of `"x, y"` points back into
/// curve control points. A 2-point identity (0,0 → 255,255) collapses to empty:
/// Lightroom ALWAYS writes the master curve (even "Linear"), while our writer
/// omits empty curves — collapsing keeps a re-import equal to a recipe that
/// never touched the curve. An element that opens but never closes is `Err`,
/// not "no curve": present-but-unreadable flows into the same disclosure as a
/// value that does not parse.
pub(super) fn parse_curve_checked(xmp: &str, tag: &str) -> Result<Vec<CurvePoint>, ()> {
    const MAX_CURVE_POINTS_FROM_XMP: usize = 256;
    let Some(body) = owned_element_body(xmp, &format!("crs:{tag}"))? else {
        return Ok(Vec::new());
    };

    let mut pts = Vec::new();
    // Items are matched by tag NAME through the shared tag scanner: the old
    // literal `"<rdf:li>"` split read a whitespace-spelled `<rdf:li >` item
    // as NO item at all — a present curve imported silently empty and the
    // next save deleted it, while the module's own standard (1719-1723,
    // owned_element_body) says present-but-unreadable must flow into
    // disclosure, never into silence.
    let mut from = 0;
    while let Some((start, end, self_closing)) = next_xml_tag(body, from) {
        let tag = &body[start..=end];
        if tag.starts_with("</") || tag_name(tag) != "rdf:li" {
            from = end + 1;
            continue;
        }
        if self_closing || pts.len() >= MAX_CURVE_POINTS_FROM_XMP {
            return Err(()); // an empty <rdf:li/> holds no "x, y" point
        }
        let close = element_close_start(body, "rdf:li", end).ok_or(())?;
        let mut it = body[end + 1..close].split(',');
        let x = it.next().ok_or(())?.trim().parse::<f32>().map_err(|_| ())?;
        let y = it.next().ok_or(())?.trim().parse::<f32>().map_err(|_| ())?;
        if it.next().is_some() || !x.is_finite() || !y.is_finite() {
            return Err(());
        }
        // Out-of-domain coordinates are as unparsable as non-finite ones:
        // silently saturating "999, -5" to (255, 0) imported a curve that
        // renders nearly black AND persisted it on the next save (16-lane
        // scan L05). Err flows into the same disclosure + drop path.
        let (x, y) = (x.round(), y.round());
        if !(0.0..=255.0).contains(&x) || !(0.0..=255.0).contains(&y) {
            return Err(());
        }
        pts.push(CurvePoint { input: x as u8, output: y as u8 });
        from = close + 1;
    }

    let identity = [CurvePoint { input: 0, output: 0 }, CurvePoint { input: 255, output: 255 }];
    Ok(if pts == identity { Vec::new() } else { pts })
}

pub(super) fn parse_curve(xmp: &str, tag: &str) -> Vec<CurvePoint> {
    parse_curve_checked(xmp, tag).unwrap_or_default()
}

/// The point colours (v1.5.0) back from `<crs:PointColors>`: one swatch per
/// `rdf:li` of nineteen numbers, Lightroom's no-swatch placeholder skipped
/// (`PointColor::from_numbers`). `Err` — present but unreadable, the curve
/// rule — for an item that is not nineteen numbers in their domains, an
/// element that never closes, or more swatches than a recipe keeps
/// ([`crate::recipe::MAX_POINT_COLORS`]): a list cut short would render some
/// of the photographer's colours and silently not the rest.
pub(super) fn parse_point_colors_checked(xmp: &str) -> Result<Vec<crate::recipe::PointColor>, ()> {
    let Some(body) = owned_element_body(xmp, "crs:PointColors")? else {
        return Ok(Vec::new());
    };
    let mut swatches = Vec::new();
    let mut from = 0;
    while let Some((start, end, self_closing)) = next_xml_tag(body, from) {
        let tag = &body[start..=end];
        if tag.starts_with("</") || tag_name(tag) != "rdf:li" {
            from = end + 1;
            continue;
        }
        if self_closing {
            return Err(()); // an empty <rdf:li/> holds no nineteen numbers
        }
        let close = element_close_start(body, "rdf:li", end).ok_or(())?;
        let numbers = body[end + 1..close]
            .split(',')
            .map(|v| v.trim().parse::<f32>().map_err(|_| ()))
            .collect::<Result<Vec<f32>, ()>>()?;
        if let Some(swatch) = crate::recipe::PointColor::from_numbers(&numbers)? {
            if swatches.len() == crate::recipe::MAX_POINT_COLORS {
                return Err(());
            }
            swatches.push(swatch);
        }
        from = close + 1;
    }
    Ok(swatches)
}

pub(super) fn parse_point_colors(xmp: &str) -> Vec<crate::recipe::PointColor> {
    parse_point_colors_checked(xmp).unwrap_or_default()
}
