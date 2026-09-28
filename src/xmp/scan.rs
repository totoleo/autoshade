//! The XML scanner: tags, attributes, the crs description, landmarks, matching closes, owned element bodies and the crs scope.

use super::*;

/// The index of the `>` ending the tag that opens at `start`, plus whether
/// the tag is self-closing. QUOTE-AWARE: attribute values may legally
/// contain `>` (Lightroom mask names do).
pub(super) fn scan_tag_end(doc: &str, start: usize) -> Option<(usize, bool)> {
    let mut quote: Option<char> = None;
    let mut prev_nonws = ' ';
    for (i, c) in doc[start..].char_indices() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => quote = Some(c),
                '>' => return Some((start + i, prev_nonws == '/')),
                _ => {}
            },
        }
        if !c.is_whitespace() {
            prev_nonws = c;
        }
    }
    None
}

pub(super) struct XmlAttribute<'a> {
    pub(super) name: &'a str,
    pub(super) value: &'a str,
    pub(super) span: std::ops::Range<usize>,
}

pub(super) fn next_xml_attribute<'a>(tag: &'a str, cursor: &mut usize) -> Option<XmlAttribute<'a>> {
    let bytes = tag.as_bytes();
    let mut i = *cursor;
    if i == 0 && bytes.first() == Some(&b'<') {
        i = 1;
        if bytes.get(i) == Some(&b'/') {
            i += 1;
        }
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && !matches!(bytes[i], b'/' | b'>')
        {
            i += 1;
        }
    }
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() || matches!(bytes[i], b'/' | b'>') {
        return None;
    }

    let start = i;
    while i < bytes.len()
        && !bytes[i].is_ascii_whitespace()
        && !matches!(bytes[i], b'=' | b'/' | b'>')
    {
        i += 1;
    }
    let name_end = i;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if bytes.get(i) != Some(&b'=') {
        return None;
    }
    i += 1;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let quote = *bytes.get(i)?;
    if !matches!(quote, b'"' | b'\'') {
        return None;
    }
    i += 1;
    let value_start = i;
    while i < bytes.len() && bytes[i] != quote {
        i += 1;
    }
    let value_end = i;
    i += 1;
    *cursor = i;
    Some(XmlAttribute {
        name: &tag[start..name_end],
        value: &tag[value_start..value_end],
        span: start..i,
    })
}

pub(super) fn xml_attribute_raw<'a>(
    tag: &'a str,
    key: &str,
) -> Option<(std::ops::Range<usize>, &'a str)> {
    let mut cursor = 0;
    while let Some(a) = next_xml_attribute(tag, &mut cursor) {
        if a.name == key {
            return Some((a.span, a.value));
        }
    }
    None
}

pub(super) fn next_xml_tag(doc: &str, mut from: usize) -> Option<(usize, usize, bool)> {
    'scan: loop {
        let start = from + doc[from..].find('<')?;
        let rest = &doc[start..];
        for &(open, close) in &CONSTRUCTS {
            if let Some(after) = rest.strip_prefix(open) {
                from = start + open.len() + after.find(close)? + close.len();
                continue 'scan;
            }
        }
        let (end, self_closing) = scan_tag_end(doc, start)?;
        return Some((start, end, self_closing));
    }
}



/// The first `rdf:Description` opening tag that carries camera-raw settings:
/// it declares `xmlns:crs`, holds a `crs:` attribute, or holds a top-level
/// `crs:` CHILD element. The child rule is what finds a Description whose
/// `xmlns:crs` lives on an ANCESTOR (`rdf:RDF`) and whose settings are all in
/// property-element form — a legal spelling the attribute-only test missed,
/// which sent the merge down the insert path and spliced a SECOND settings
/// Description into the same document. Depth is what keeps the child rule
/// honest: a `crs:` element nested inside a foreign container marks its OWN
/// parent Description, never an outer one — the parent is whatever element is
/// open when the `crs:` child appears (single pass, one open-element stack),
/// so a creative Look's baked parameters can only ever mark the settings
/// Description that contains the Look, which is the right answer anyway.
pub(super) fn find_crs_description(doc: &str) -> Option<usize> {
    // (name, open-tag start) for every open element. Close-tag mismatches pop
    // nothing — malformed markup degrades to the old attribute-only rule
    // instead of failing a document the flat scan used to find.
    let mut stack: Vec<(&str, usize)> = Vec::new();
    let mut from = 0;
    while let Some((start, end, self_closing)) = next_xml_tag(doc, from) {
        let tag = &doc[start..=end];
        let name = tag_name(tag);
        if tag.starts_with("</") {
            if stack.last().is_some_and(|(n, _)| *n == name) {
                stack.pop();
            }
            from = end + 1;
            continue;
        }
        if name == "rdf:Description" {
            let mut cursor = 0;
            while let Some(a) = next_xml_attribute(tag, &mut cursor) {
                // The declaration only marks the settings Description when it
                // binds the CANONICAL camera-raw URI. The scope-aware gate
                // (R12-03) now lets an UNUSED foreign rebind through as
                // harmless — but the merge keys on this very attribute, and
                // splicing canonical-intent `crs:` settings into a scope
                // where `crs` means something else would corrupt the
                // document the gate just cleared.
                if (a.name == "xmlns:crs" && xml_unescape(a.value).as_ref() == CRS_URI)
                    || a.name.starts_with("crs:")
                {
                    return Some(start);
                }
            }
        } else if name.starts_with("crs:")
            && let Some(&(parent, parent_start)) = stack.last()
            && parent == "rdf:Description"
        {
            return Some(parent_start);
        }
        if !self_closing {
            stack.push((name, start));
        }
        from = end + 1;
    }
    None
}

pub(super) const CRS_URI: &str = "http://ns.adobe.com/camera-raw-settings/1.0/";
const RDF_URI: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

/// Every scanner in this module identifies namespaces by the CONVENTIONAL
/// prefixes (`crs:`, `rdf:`) — never by URI. A document that binds either
/// namespace to a different prefix, or binds `crs`/`rdf` to a different URI,
/// is therefore one these scanners silently misread: its settings import as
/// neutral with no disclosure, and the merge — finding "no" crs Description —
/// used to splice OUR settings in beside the foreign-prefixed ones, publishing
/// one document with two contradictory camera-raw blocks and a clean "saved".
/// This is the refusal gate: `Some(reason)` names the binding, the merge
/// refuses (the caller regenerates AND discloses), and the import surfaces the
/// same sentence.
///
/// SCOPE-AWARE (R12-03): bindings are resolved through an element scope
/// stack, XML-semantics style, and the gate fires only where a binding would
/// actually corrupt this document's reading — a `crs:`/`rdf:` NAME (element
/// or attribute) whose in-scope binding is not the canonical URI, a name
/// under some OTHER prefix whose in-scope binding IS a canonical URI, or an
/// unprefixed ELEMENT under a default namespace bound to one (unprefixed
/// attributes take no namespace, per XML). A declaration nobody uses — a
/// nested island rebinding `crs` around content that never says `crs:`, or a
/// foreign alias for the camera-raw URI that no name ever resolves through —
/// no longer refuses the whole document the way the flat scan did. An
/// undeclared `crs:`/`rdf:` prefix still passes: the scanners read by
/// prefix and never required a declaration.
pub(super) fn xmlns_conflict(doc: &str) -> Option<String> {
    if doc.len() > MAX_XMP_BYTES {
        return None;
    }
    // One frame per OPEN element that declares namespaces, tagged with its
    // depth AND its element name: a close tag pops a frame only when both
    // match, so a surplus or misnamed close (round-13 review R13-01) leaves
    // the frame in place — malformed nesting degrades toward refusal, never
    // toward releasing a foreign binding early. `""` keys the default
    // namespace. The bound counts LIVE DECLARATIONS, not frames (R13-02): a
    // single tag can carry a declaration flood, and `resolve` walks every
    // live declaration per name, so the budget is what keeps an adversarial
    // 16 MiB document from going quadratic.
    const MAX_NS_DECLS: usize = 256;
    struct NsFrame<'a> {
        depth: usize,
        name: &'a str,
        decls: Vec<(&'a str, String)>,
    }
    fn resolve<'a>(frames: &'a [NsFrame<'_>], pfx: &str) -> Option<&'a str> {
        frames.iter().rev().find_map(|f| {
            f.decls.iter().rev().find(|(p, _)| *p == pfx).map(|(_, u)| u.as_str())
        })
    }
    fn against(uri: &str, pfx: Option<&str>) -> Option<String> {
        match pfx {
            Some("crs") => (uri != CRS_URI)
                .then(|| format!("xmlns:crs is bound to {uri}, not the camera-raw namespace")),
            Some("rdf") => (uri != RDF_URI)
                .then(|| format!("xmlns:rdf is bound to {uri}, not the RDF namespace")),
            Some(pfx) if uri == CRS_URI => Some(format!(
                "the camera-raw namespace is bound to the `{pfx}:` prefix; \
                 this build reads only `crs:`"
            )),
            Some(pfx) if uri == RDF_URI => Some(format!(
                "the RDF namespace is bound to the `{pfx}:` prefix; \
                 this build reads only `rdf:`"
            )),
            None if uri == CRS_URI || uri == RDF_URI => Some(format!(
                "the {} namespace is bound as the DEFAULT namespace; this \
                 build reads only the `crs:`/`rdf:` prefixes",
                if uri == CRS_URI { "camera-raw" } else { "RDF" }
            )),
            _ => None,
        }
    }
    let mut depth: usize = 0;
    let mut live_decls: usize = 0;
    let mut frames: Vec<NsFrame> = Vec::new();
    let mut from = 0;
    while let Some((start, end, self_closing)) = next_xml_tag(doc, from) {
        from = end + 1;
        let tag = &doc[start..=end];
        if tag.starts_with("</") {
            if depth > 0 {
                if frames.last().is_some_and(|f| f.depth == depth && f.name == tag_name(tag)) {
                    let f = frames.pop().expect("just matched");
                    live_decls -= f.decls.len();
                }
                depth -= 1;
            }
            continue;
        }
        depth += 1;
        // Declarations bind the element they sit on (and its own other
        // attributes) regardless of attribute order — collect them first.
        let mut decls: Vec<(&str, String)> = Vec::new();
        let mut cursor = 0;
        while let Some(a) = next_xml_attribute(tag, &mut cursor) {
            if let Some(pfx) = a.name.strip_prefix("xmlns:") {
                decls.push((pfx, xml_unescape(a.value).into_owned()));
            } else if a.name == "xmlns" {
                decls.push(("", xml_unescape(a.value).into_owned()));
            }
        }
        let name = tag_name(tag);
        if !decls.is_empty() {
            live_decls += decls.len();
            frames.push(NsFrame { depth, name, decls });
            if live_decls > MAX_NS_DECLS {
                // Beyond the tracking budget the gate cannot prove a binding
                // harmless (or resolve names affordably), so it refuses —
                // conservative, and disclosed.
                return Some(
                    "more xmlns declarations than this build tracks; \
                     namespace bindings cannot be verified"
                        .to_string(),
                );
            }
        }
        // The element's own name…
        let elem_pfx = name.split_once(':').map(|(p, _)| p);
        if let Some(uri) = resolve(&frames, elem_pfx.unwrap_or(""))
            && let Some(why) = against(uri, elem_pfx)
        {
            return Some(why);
        }
        // …then every non-declaration attribute name. Unprefixed attributes
        // take no namespace (not the default one), so only prefixed names
        // resolve here.
        let mut cursor = 0;
        while let Some(a) = next_xml_attribute(tag, &mut cursor) {
            if a.name == "xmlns" || a.name.starts_with("xmlns:") {
                continue;
            }
            if let Some((pfx, _)) = a.name.split_once(':')
                && let Some(uri) = resolve(&frames, pfx)
                && let Some(why) = against(uri, Some(pfx))
            {
                return Some(why);
            }
        }
        if self_closing {
            if frames.last().is_some_and(|f| f.depth == depth) {
                let f = frames.pop().expect("just matched");
                live_decls -= f.decls.len();
            }
            depth -= 1;
        }
    }
    None
}



/// The text constructs whose contents are NOT markup. A `</rdf:Description>`
/// inside any of them is not a close.
pub(super) const CONSTRUCTS: [(&str, &str); 3] = [("<!--", "-->"), ("<![CDATA[", "]]>"), ("<?", "?>")];

/// Every landmark this scanner needs, each cached as "the first hit AT OR
/// AFTER `from`" and refreshed only once `from` has passed it.
///
/// This is the whole performance contract. `from` only ever moves forward, so
/// a cached hit stays correct until it is crossed, and each refresh resumes
/// its scan at the new `from` — every landmark therefore sweeps the document
/// at most once and the loop is linear overall. A cursor that has run out
/// (`None`) is never searched again: since `from` only advances, a pattern
/// with no occurrence after `from` has none after any later `from` either.
/// That last rule is load-bearing — re-searching an absent pattern to the end
/// of the document on every iteration is itself quadratic.
struct Landmarks {
    /// First `</rdf:Description>` at or after `from` — required, so not optional.
    close: usize,
    /// First `<rdf:Description` at or after `from`, if any remain.
    open: Option<usize>,
    /// First occurrence of each entry of [`CONSTRUCTS`], if any remain.
    ctor: [Option<usize>; 3],
}

impl Landmarks {
    fn new(doc: &str, from: usize) -> Option<Self> {
        const CLOSE: &str = "</rdf:Description>";
        const OPEN: &str = "<rdf:Description";
        Some(Landmarks {
            close: from + doc[from..].find(CLOSE)?,
            open: doc[from..].find(OPEN).map(|r| from + r),
            ctor: std::array::from_fn(|i| doc[from..].find(CONSTRUCTS[i].0).map(|r| from + r)),
        })
    }

    /// Advance every cursor the new `from` has overtaken. Returns `None` when
    /// no close remains, which sinks the whole scope.
    fn refresh(&mut self, doc: &str, from: usize) -> Option<()> {
        const CLOSE: &str = "</rdf:Description>";
        const OPEN: &str = "<rdf:Description";
        if from > self.close {
            self.close = from + doc[from..].find(CLOSE)?;
        }
        if self.open.is_some_and(|o| from > o) {
            self.open = doc[from..].find(OPEN).map(|r| from + r);
        }
        for (slot, (open, _)) in self.ctor.iter_mut().zip(CONSTRUCTS.iter()) {
            // A cursor already at or ahead of `from` is still the first hit;
            // one that has run out (None) stays out, and re-searching it would
            // sweep to the end of the document on every iteration — the exact
            // shape of the quadratic this cache exists to remove.
            if slot.is_some_and(|p| from > p) {
                *slot = doc[from..].find(open).map(|r| from + r);
            }
        }
        Some(())
    }

    /// The construct that opens before both the next open tag and the close —
    /// the only one that is this iteration's business.
    fn pending_construct(&self) -> Option<(usize, &'static str, &'static str)> {
        self.ctor
            .iter()
            .zip(CONSTRUCTS.iter())
            .filter_map(|(slot, (open, close))| slot.map(|p| (p, *open, *close)))
            .filter(|(p, _, _)| *p < self.close && self.open.is_none_or(|o| *p < o))
            .min_by_key(|(p, _, _)| *p)
    }
}

/// The `</rdf:Description>` closing the element whose opening tag ended just
/// before `from` — DEPTH-COUNTED, because Lightroom nests `rdf:Description`
/// elements inside mask corrections (the batch-3 lesson: naive scans shred
/// nested structures).
pub(super) fn find_matching_close(doc: &str, mut from: usize) -> Option<usize> {
    const CLOSE: &str = "</rdf:Description>";
    let mut depth = 0usize;
    // Two DISTINCT quadratic blowups have lived in this function; both showed
    // up as a sidecar beside a RAW pegging a core inside SAVE_LOCK, holding
    // one of the server's eight request permits, on nothing worse than photo
    // SELECTION. Both are now answered by the same rule — cache every
    // landmark, never re-scan what `from` has not passed (see [`Landmarks`]).
    //
    //   1. Re-running the CLOSE search on every nested open: Θ(depth²).
    //      Measured 2.8 MB of nesting at 55.97 s.
    //   2. Re-running the CONSTRUCT search on every skipped comment / PI /
    //      CDATA — the scan that FIXED (1) introduced this one, and it was
    //      worse per byte because a body of back-to-back comments re-scanned
    //      the whole remaining window three times per construct while `from`
    //      crawled forward one construct at a time. Measured 640 KB of
    //      comments at 8.47 s, against 51 µs before the construct skip
    //      existed at all.
    //
    // Both shapes are now linear: 2.8 MB nested and 640 KB of comments each
    // finish in single-digit milliseconds (see the timed test).
    let mut marks = Landmarks::new(doc, from)?;
    loop {
        marks.refresh(doc, from)?;
        // Comments / PIs / CDATA are TEXT. `crs_scope_inner` and
        // `top_level_owned_spans` already skip all three; this scanner did
        // not, so a sidecar carrying `</rdf:Description>` in a comment
        // reported a bogus close, the body came back truncated mid-construct,
        // and the whole merge fell back to a fresh document — dropping every
        // Lightroom-only property it exists to preserve. (Attribute values
        // cannot trigger it: raw `<` is illegal there in XML, and
        // `scan_tag_end` is quote-aware regardless.)
        if let Some((at, open, close)) = marks.pending_construct() {
            let body_at = at + open.len();
            // An UNTERMINATED construct walks `from` off the end, so the next
            // refresh finds no close and the scope sinks to the whole-document
            // fallback: unbalanced markup is never silently read as tags.
            from = match doc[body_at..].find(close) {
                Some(end_rel) => body_at + end_rel + close.len(),
                None => doc.len(),
            };
            continue;
        }
        match marks.open {
            Some(open_at) if open_at < marks.close => {
                let (end, self_closing) = scan_tag_end(doc, open_at)?;
                if !self_closing {
                    depth += 1;
                }
                from = end + 1;
            }
            _ => {
                if depth == 0 {
                    return Some(marks.close);
                }
                depth -= 1;
                from = marks.close + CLOSE.len();
            }
        }
    }
}

/// Re-stamp the AutoShade rationale comment (older saves embedded it) so a
/// merged document never carries a STALE rationale for a new recipe.
pub(super) fn refresh_rationale_comment(doc: String, r: &EditRecipe) -> String {
    const MARK: &str = "<!-- Generated by AutoShade. AI rationale: ";
    // Also an ON-DISK token, and it does NOT retire with the `AUTOSHOP_*`
    // environment door v1.2.4 closed. That door stood in the user's
    // CONFIGURATION, which this app rewrites on the next save, so it could
    // expire. This mark stands in the user's DATA — sidecars written before the
    // rename — which this app rewrites only when it can FIND the mark, so there
    // is no release after which those bytes go away. Dropping the spelling
    // would retire nothing: it would leave the OLD reasoning attached to a NEW
    // recipe and append a second comment beside it, the exact defect this
    // function exists to prevent (`a_pre_rename_rationale_comment_is_still_refreshed`
    // is the proof). Found under either spelling, always rewritten under the
    // current one.
    const MARK_PRE_RENAME: &str = "<!-- Generated by Autoshop. AI rationale: ";
    let Some(start) = doc.find(MARK).or_else(|| doc.find(MARK_PRE_RENAME)) else { return doc };
    let Some(end) = doc[start..].find("-->") else { return doc };
    format!(
        "{}{MARK}{} (confidence {:.2}) {}",
        &doc[..start],
        safe_rationale(r),
        r.confidence,
        &doc[start + end..]
    )
}

/// The element name in a start or end tag: `<crs:Exposure2012 xml:lang="…">`
/// and `</crs:Exposure2012>` both give `crs:Exposure2012`.
pub(super) fn tag_name(tag: &str) -> &str {
    let t = tag.trim_start_matches('<').trim_start_matches('/');
    let end = t.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(t.len());
    &t[..end]
}

/// The start of the close tag ending the element whose open tag ends at
/// `open_gt` — matched by NAME through [`next_xml_tag`], so the
/// whitespace-carrying close (`</crs:Key >`, legal XML) and closes quoted
/// inside comments/CDATA/PIs are both handled, and same-name nesting is
/// depth-counted. `None` = the element never closes.
pub(super) fn element_close_start(doc: &str, name: &str, open_gt: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut from = open_gt + 1;
    while let Some((start, end, self_closing)) = next_xml_tag(doc, from) {
        let tag = &doc[start..=end];
        if tag_name(tag) == name {
            if tag.starts_with("</") {
                if depth == 0 {
                    return Some(start);
                }
                depth -= 1;
            } else if !self_closing {
                depth += 1;
            }
        }
        from = end + 1;
    }
    None
}

/// The body of the first `<{name}>…</{name}>` element in `scope`, matched by
/// tag NAME — so the attribute-carrying spelling
/// (`<crs:ToneCurvePV2012 xml:lang="x-default">`) and a whitespace close both
/// resolve to the same property, exactly as the writer's own strip does
/// (see [`top_level_owned_spans`]; the literal predecessor here was the
/// reader's last exact-string holdout, and every miss read as "absent").
///
/// `Ok(None)` = no such element. `Err(())` = the element OPENS but never
/// closes — present-but-unreadable, which callers must disclose rather than
/// fold into "absent" (a curve that cannot be read imports as a silent
/// neutral, and the next save persists the neutral).
pub(crate) fn owned_element_body<'a>(scope: &'a str, name: &str) -> Result<Option<&'a str>, ()> {
    Ok(owned_element_body_span(scope, name)?.map(|(a, b)| &scope[a..b]))
}

/// [`owned_element_body`]'s answer as a byte SPAN `[start, end)` into `scope`,
/// with the same three verdicts and the same reasons.
///
/// Exists because a caller that has to hand offsets back in the ORIGINAL
/// string's coordinates cannot use a subslice: `base_geometry_at` returns a
/// position inside the whole correction segment while it must SEARCH only the
/// component list (R27 Batch-4 hazard 2), and re-deriving the offset from a
/// slice's address is pointer arithmetic dressed up as parsing.
/// [`owned_element_body`] is written in terms of this one so the two can never
/// answer differently.
pub(super) fn owned_element_body_span(scope: &str, name: &str) -> Result<Option<(usize, usize)>, ()> {
    let mut from = 0;
    while let Some((start, end, self_closing)) = next_xml_tag(scope, from) {
        let tag = &scope[start..=end];
        if !tag.starts_with("</") && tag_name(tag) == name {
            if self_closing {
                // An empty body, spelled as the empty span just past the tag —
                // `&scope[end+1..end+1]` is `""`, which is what the slicing
                // form has always returned here.
                return Ok(Some((end + 1, end + 1)));
            }
            return match element_close_start(scope, name, end) {
                Some(close) => Ok(Some((end + 1, close))),
                None => Err(()),
            };
        }
        from = end + 1;
    }
    Ok(None)
}

/// One `crs:What`-carrying component found by [`components_in`].
///
/// The `crs:What` may sit on the `<rdf:li>` itself (Lightroom's attribute form
/// for parametric shapes) or on an `<rdf:Description>` inside it (the element
/// form every `Mask/Aggregate`, `Mask/Paint` and `Mask/Image` uses). Both are
/// the same thing to every caller here, so both arrive as one struct.
pub(super) struct XmlComponent<'a> {
    /// The tag that carries `crs:What`, `<` to `>` inclusive.
    pub(super) tag: &'a str,
    /// `<` offset of that tag inside the walked body.
    pub(super) start: usize,
    /// `>` offset of that tag inside the walked body.
    pub(super) gt: usize,
    self_closing: bool,
    /// How many component elements are OPEN above this one: `0` = a direct
    /// member of the walked list, `1` = a member of a list nested inside one
    /// component (a `Mask/Paint` inside a `Mask/Aggregate`'s `crs:Masks`), and
    /// so on.
    pub(super) depth: usize,
    pub(super) what: std::borrow::Cow<'a, str>,
}

/// Every `crs:What` component in `body`, in document order, each tagged with
/// its NESTING DEPTH — the walk R27 Batch-4 replaced a flat scan with, and the
/// reason it had to.
///
/// `classify_correction` used to walk the mask block with a bare
/// [`next_xml_tag`] loop, which sees a `Mask/Paint` inside a `Mask/Aggregate`
/// as a SIBLING of it. That was harmless only while both answers were "refuse
/// the correction": the moment a Paint means something, a flat walk
/// double-counts every stroke as a top-level component and loses the group's
/// own blend mode. Nesting-awareness is step 0 of the brush arm, not a
/// tidy-up.
///
/// Malformed markup degrades toward REFUSAL rather than toward a wrong answer:
/// a close tag that does not match the innermost open element pops nothing
/// (the same recovery [`find_crs_description`] takes), so depth stays high and
/// the components below it read as nested — which every caller here refuses.
pub(super) fn components_in(body: &str) -> Vec<XmlComponent<'_>> {
    let mut out: Vec<XmlComponent<'_>> = Vec::new();
    // (element name, does it contribute a level of component depth?)
    let mut stack: Vec<(&str, bool)> = Vec::new();
    let mut depth = 0usize;
    let mut from = 0;
    while let Some((start, gt, self_closing)) = next_xml_tag(body, from) {
        let tag = &body[start..=gt];
        from = gt + 1;
        let name = tag_name(tag);
        if tag.starts_with("</") {
            if stack.last().is_some_and(|(n, _)| *n == name)
                && let Some((_, was_component)) = stack.pop()
                && was_component
            {
                depth -= 1;
            }
            continue;
        }
        let what = xml_attribute_raw(tag, "crs:What").map(|(_, raw)| xml_unescape(raw));
        if let Some(w) = &what {
            out.push(XmlComponent {
                tag,
                start,
                gt,
                self_closing,
                depth,
                what: w.clone(),
            });
        }
        if !self_closing {
            stack.push((name, what.is_some()));
            if what.is_some() {
                depth += 1;
            }
        }
    }
    out
}

/// The BODY of one component's own element — `Ok(None)` when the component is
/// self-closing (Lightroom's attribute form, which has no body by
/// construction), `Err(())` when it opens and never closes.
pub(super) fn component_body<'a>(scope: &'a str, c: &XmlComponent<'_>) -> Result<Option<&'a str>, ()> {
    if c.self_closing {
        return Ok(None);
    }
    match element_close_start(scope, tag_name(c.tag), c.gt) {
        Some(close) => Ok(Some(&scope[c.gt + 1..close])),
        None => Err(()),
    }
}

/// Byte spans of the body's TOP-LEVEL owned property elements, in reverse
/// document order so the caller can splice them out without re-indexing.
///
/// DEPTH-AWARE, and matched by tag NAME. A flat `<crs:Name>` literal scan
/// reached INSIDE the creative Look this merge exists to preserve: Adobe
/// writes a profile's baked parameters as owned-LOOKING children of a nested
/// `rdf:Description` (`<crs:Look><rdf:Description><crs:Parameters>
/// <rdf:Description><crs:Exposure2012>…`), and stripping those gutted the
/// Look — verified by a probe on that exact shape. An owned property belongs
/// to THIS Description; anything deeper belongs to its container. Matching by
/// name also catches the attribute-carrying spelling
/// (`<crs:Exposure2012 xml:lang="x-default">`), which the literal missed —
/// leaving behind the very duplicate the element strip exists to prevent.
///
/// `None` = markup this scanner cannot account for (an unterminated tag, a
/// close with no open, an owned element that never closes). The merge then
/// bails and the caller regenerates the document, which is the pre-merge
/// behaviour.
pub(super) fn top_level_owned_spans(
    body: &str,
    owned: &std::collections::HashSet<String>,
) -> Option<Vec<(usize, usize)>> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut depth = 0usize;
    // The owned element currently open AT TOP LEVEL: (span start, tag name).
    let mut open: Option<(usize, String)> = None;
    let mut i = 0usize;
    while let Some(rel) = body[i..].find('<') {
        let p = i + rel;
        let rest = &body[p..];
        if let Some(after) = rest.strip_prefix("<!--") {
            i = p + 4 + after.find("-->")? + 3;
            continue;
        }
        if let Some(after) = rest.strip_prefix("<?") {
            i = p + 2 + after.find("?>")? + 2;
            continue;
        }
        // CDATA is TEXT, not markup: its `<`/`>` must not be counted as tags.
        // Counting them left `depth` unbalanced, which bails the whole merge
        // into a full regenerate — and that path replaces the user's sidecar
        // with our own document, taking every foreign property with it. Legal
        // XML must never reach the bail.
        if let Some(after) = rest.strip_prefix("<![CDATA[") {
            i = p + "<![CDATA[".len() + after.find("]]>")? + 3;
            continue;
        }
        let (gt, self_closing) = scan_tag_end(body, p)?;
        let name = tag_name(&body[p..=gt]).to_string();
        if rest.starts_with("</") {
            // A close with no open: not markup this scanner can splice.
            depth = depth.checked_sub(1)?;
            if depth == 0
                && let Some((start, open_name)) = open.take()
            {
                if name != open_name {
                    return None;
                }
                spans.push((start, gt + 1));
            }
            i = gt + 1;
            continue;
        }
        if depth == 0
            && (name.strip_prefix("crs:").is_some_and(|bare| owned.contains(bare))
                // …or a FULL name: the payload's `<asr:Rasters>`, the one
                // owned element outside the crs namespace.
                || owned.contains(&name))
        {
            // The leading indentation (and the newline before it) goes with
            // the property — the same whitespace hygiene the attribute strip
            // applies, so an untouched document's formatting is preserved.
            let mut start = p;
            while start > 0 && matches!(body.as_bytes()[start - 1], b' ' | b'\t') {
                start -= 1;
            }
            if start > 0 && body.as_bytes()[start - 1] == b'\n' {
                start -= 1;
            }
            if self_closing {
                spans.push((start, gt + 1));
            } else {
                open = Some((start, name.clone()));
            }
        }
        if !self_closing {
            depth += 1;
        }
        i = gt + 1;
    }
    if open.is_some() || depth != 0 {
        return None; // unbalanced: regenerate rather than splice blind
    }
    spans.reverse();
    Some(spans)
}

/// The crs Description's OWN property scope — the text every whole-document
/// READ below must be restricted to: its opening tag plus the top-level
/// children that carry ITS settings.
///
/// The mirror of [`top_level_owned_spans`], for the reader. Adobe writes a
/// creative profile's baked parameters as owned-LOOKING children of a NESTED
/// `rdf:Description` (`<crs:Look><rdf:Description><crs:Parameters>
/// <rdf:Description><crs:Clarity2012>+50…`), and the scanners here
/// ([`crs_str`], [`parse_curve`], [`parse_masks`]) match by name anywhere in
/// the string they are given. Whenever the top-level Description OMITS a key
/// the Look nests, the flat scan therefore answered from the profile — the
/// import turned a camera profile's baked look into user slider values, and
/// the next save persisted them. The WRITER's depth-aware strip exists for
/// exactly this shape; the reader now shares the rule.
///
/// A top-level child that nests an `rdf:Description` is a CONTAINER of
/// someone else's settings and is dropped. `crs:MaskGroupBasedCorrections`
/// nests them too but IS this Description's own property (its nested
/// Descriptions are its mask items), so it is kept by name — dropping it
/// would blind [`parse_masks`].
///
/// `None` = markup this scanner cannot account for; [`crs_own_scope`] then
/// hands back the whole document, which is exactly the pre-fix behaviour.
pub(super) fn crs_scope_inner(doc: &str) -> Option<String> {
    /// Owned containers that legitimately nest `rdf:Description`.
    const KEEP_NESTED: [&str; 1] = ["crs:MaskGroupBasedCorrections"];
    let start = find_crs_description(doc)?;
    element_own_scope(doc, start, |name, nests_description| {
        !nests_description || KEEP_NESTED.contains(&name)
    })
}

/// One CORRECTION's own property scope — the same law, one level down (R28
/// Batch-5 5d).
///
/// A `crs:What="Correction"` element carries its sliders as attributes on its
/// own tag and its four point curves as child elements, and it carries its
/// COMPONENTS inside `crs:CorrectionMasks`. Every slider read used to scan the
/// whole correction segment, so an attribute named `crs:LocalExposure2012` on a
/// nested `Mask/Paint` — or on anything else inside that component list —
/// answered for the correction whenever the correction itself omitted the key.
///
/// Zero real Lightroom files do that (LR writes the Local* family on the
/// Correction tag and nothing else carries those names), which is why the
/// adjudication rated it LOW and adversarial-input-only. But the nearest real
/// threat is already in the corpus: Lightroom really does put `crs:Local*`
/// NAMES on nested `Mask/Image` components (`LocalInputDigest` and friends, 105
/// measured instances), and today's reader survives that only because those
/// values are strings nobody parses as a number — a coincidence, not a guard.
///
/// The member rule is the difference from [`crs_scope_inner`]: at the top level
/// a child is foreign when it NESTS a settings block; inside a correction the
/// component list is foreign BY NAME, because its members are `rdf:li`s that may
/// carry no `rdf:Description` at all (Lightroom's attribute form for parametric
/// shapes) and would otherwise stay in scope.
pub(super) fn correction_own_scope(seg: &str) -> std::borrow::Cow<'_, str> {
    let start = match next_xml_tag(seg, 0) {
        Some((s, _, _)) => s,
        None => return std::borrow::Cow::Borrowed(seg),
    };
    match element_own_scope(seg, start, |name, nests_description| {
        !nests_description && name != "crs:CorrectionMasks"
    }) {
        Some(s) => std::borrow::Cow::Owned(s),
        // Markup this scanner cannot account for: fall back to the whole
        // segment, which is the pre-5d behaviour and the same direction
        // `crs_own_scope` degrades in. A correction this malformed is already
        // heading for a refusal in `classify_correction`.
        None => std::borrow::Cow::Borrowed(seg),
    }
}

/// The shared walk behind [`crs_scope_inner`] and [`correction_own_scope`]: an
/// element's opening tag plus the top-level children `keep` accepts, with
/// everything else dropped. `keep(child element name, does it nest an
/// `rdf:Description`)`.
///
/// TWO membership rules on ONE walk, deliberately — the same shape R28 Batch-3
/// gave the rotate/storage raster sets. The alternative was a second copy of
/// this scanner with one predicate changed, which is how the reader's scope law
/// came to hold at the document level and not at the correction level in the
/// first place.
fn element_own_scope(
    doc: &str,
    start: usize,
    keep: impl Fn(&str, bool) -> bool,
) -> Option<String> {
    let (gt, self_closing) = scan_tag_end(doc, start)?;
    let mut out = doc[start..=gt].to_string();
    if self_closing {
        return Some(out); // every setting is an attribute — no children at all
    }
    let close = find_matching_close(doc, gt + 1)?;
    let body = &doc[gt + 1..close];
    let mut depth = 0usize;
    // The top-level child currently open: (span start, tag name, nests an
    // rdf:Description).
    let mut open: Option<(usize, String, bool)> = None;
    let mut i = 0usize;
    while let Some(rel) = body[i..].find('<') {
        let p = i + rel;
        let rest = &body[p..];
        // Comments / PIs / CDATA are TEXT — their `<`/`>` are not tags (the
        // same three skips top_level_owned_spans makes, for the same reason:
        // counting them unbalances `depth` and bails the whole scope).
        if let Some(after) = rest.strip_prefix("<!--") {
            i = p + 4 + after.find("-->")? + 3;
            continue;
        }
        if let Some(after) = rest.strip_prefix("<?") {
            i = p + 2 + after.find("?>")? + 2;
            continue;
        }
        if let Some(after) = rest.strip_prefix("<![CDATA[") {
            i = p + "<![CDATA[".len() + after.find("]]>")? + 3;
            continue;
        }
        let (gt2, self_closing) = scan_tag_end(body, p)?;
        let name = tag_name(&body[p..=gt2]).to_string();
        if rest.starts_with("</") {
            depth = depth.checked_sub(1)?; // a close with no open: unaccountable
            if depth == 0
                && let Some((s, open_name, nested)) = open.take()
            {
                // CROSSED NAMES (`<crs:Look>…</crs:Foo>`): the tag counts
                // balance, the names do not, so this child is markup we cannot
                // account for. It is DROPPED, not bailed on — R25 P8. Bailing
                // returned `None`, and `crs_own_scope`'s `None` hands the
                // scanners THE WHOLE DOCUMENT, which promotes a creative
                // Look's baked `crs:Clarity2012` to a user slider value: the
                // precise defect this function exists to prevent, reachable
                // through its own error path. The writer's mirror
                // (`top_level_owned_spans`) does not bail on this shape at all
                // — it only tracks OWNED children — so the merge went ahead
                // while the read went wrong, and the asymmetry was the bug.
                // Dropping is the safe direction for a READ scope: the worst
                // it can do is leave a property unread, and it can never let a
                // nested settings block answer for the top level.
                if name == open_name && keep(&open_name, nested) {
                    out.push('\n');
                    out.push_str(&body[s..=gt2]);
                }
            }
            i = gt2 + 1;
            continue;
        }
        if depth == 0 {
            if self_closing {
                // No children to inspect — a bare property element is ours,
                // unless the member rule refuses it by NAME (an empty
                // `<crs:CorrectionMasks/>` is still the component list).
                if keep(&name, false) {
                    out.push('\n');
                    out.push_str(&body[p..=gt2]);
                }
            } else {
                open = Some((p, name.clone(), false));
            }
        } else if name == "rdf:Description"
            && let Some(o) = open.as_mut()
        {
            o.2 = true; // this top-level child nests a foreign settings block
        }
        if !self_closing {
            depth += 1;
        }
        i = gt2 + 1;
    }
    if open.is_some() || depth != 0 {
        return None;
    }
    Some(out)
}

/// [`crs_scope_inner`] with the whole-document fallback — what every reader
/// that is handed a complete sidecar must pass to the scanners. Borrowed on
/// the fallback path, so an unmergeable/unparseable document costs nothing.
pub(crate) fn crs_own_scope(xmp: &str) -> std::borrow::Cow<'_, str> {
    if xmp.len() > MAX_XMP_BYTES {
        return std::borrow::Cow::Borrowed("");
    }
    match crs_scope_inner(xmp) {
        Some(s) => std::borrow::Cow::Owned(s),
        None => std::borrow::Cow::Borrowed(xmp),
    }
}
