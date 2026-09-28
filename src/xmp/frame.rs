//! The mask frame: Lightroom's stored frame scale, the frame aspect and scope, the declared orientation it is honoured in, and the fallbacks.

use super::*;

// ---------------------------------------------------------------------------
// The radial ellipse — Lightroom's rotated-corner box ⇄ this engine's bbox
// ---------------------------------------------------------------------------
//
// v0.32.0. Every constant and every formula below is MEASURED, on the user's
// own twelve-frame controlled Lightroom experiment plus pixel measurement of
// the exports (evidence: `lr-experiment/` in the R25 materials ledger, outside the tree,
// `probe4/PROBE4-FINAL.md` §4 is the settled statement, `probe3/
// PROBE3-ADDENDUM.md` §3 the falloff, `probe2/PROBE2-VERDICT.md` §5 the
// eight-frame table, `BBOX-DECODE.md` §2 the corner model's own statistics).
// Nothing here is inferred from a family pattern; where a value is still
// unmeasured it is named as such at the site that uses it.

/// The affine constant Lightroom applies between a radial's stored normalised
/// coordinates and the frame it draws them in: `x_px = W·(k·n − (k−1)/2)`, i.e.
/// the mask lives in a frame `k`× the export, CONCENTRIC with it.
///
/// **MEASURED**, not assumed. `P48` (`Feather="0"`, so the rendered edge
/// IS the ellipse) puts the semi-axis scale at 1.0326 ± 0.0004 by three
/// independent methods (`PROBE2-VERDICT.md` §3.1); `P47` — a hard-edged
/// mask whose centre sits 2799 px from the frame centre, which is what
/// separates "scale the axes" from "scale the frame" — puts the map at
/// 1.0315 ± 0.0005 and lands on this value to **3 px on a 2799 px lever**
/// (`PROBE4-FINAL.md` §2.1). "The centre stays put" misses by 88 px and is
/// dead twice over.
///
/// ~~ONE loose end, registered by `PROBE4-FINAL.md` §3 and deliberately not
/// modelled: the SEMI-AXIS scale measures 1.0325 on one hard-edge frame and
/// 1.0065 on another, so a single affine cannot be exactly right for both.~~
/// **The registered TRIGGER FIRED 2026-08-19 (R27 Batch-8)** — the user shot
/// exactly the named exports (centred `Feather = 0`, small at 24 mm, large at
/// 105 mm, plus a third at 34 mm) — **and the answer is that this constant is
/// a PER-FRAME quantity, not a constant** (`batch8-report.md` §4). Measured
/// per-frame scale `s`: 0.98395/0.98398 (24 mm), 0.99956/0.99953 (105 mm),
/// 1.00428/1.00398 (34 mm) — `kx` and `ky` agree to 3e-5 within every frame,
/// ellipse-fit residual 0.07–1.16 px on 720 rays, confirmed by a second
/// estimator sharing no code. The "axis vs centre scale split" (old L-05)
/// DISSOLVES: one scale about the frame centre reproduces centre AND both
/// axes on all three frames to ≤ 2.2 px, where this constant misses by
/// 30–104 px. PROBE2/PROBE4's 1.0325/1.0315 remain valid — as THOSE frames'
/// `s`. Mechanism, AS BATCH-8 LEFT IT: open — the lens-profile hypothesis had
/// the right sign and magnitude on all five frames but looked contradicted by
/// radial uniformity (`s` behaves as a pure similarity, which distortion is
/// not). It did not stay open: the next paragraph is Batch-10 measuring the
/// mechanism and dissolving that objection.
///
/// ~~WHY THE VALUE STILL STANDS UNCHANGED: with no mechanism there is nothing
/// principled to derive `s` from…~~ **The trigger above ALSO fired, same day
/// (R27 Batch-10, `batch10-report.md` §5): the mechanism is Adobe's
/// LENS-PROFILE DISTORTION.** Toggling `LensProfileEnable` 1→0 on the same
/// capture and radial moves the implied scale 0.98396 → 0.99826 (batch-8's
/// own edge finder, unchanged); independently 11 disjoint brush dabs are
/// displaced PURELY RADIALLY by `dr = −0.02487·r + 2.285e−9·r³` (rms 2.94 px;
/// any pure scale refuted at 11×, this constant at 30×). Batch-8's
/// "pure similarity" counter-argument dissolves: over one mask's narrow
/// annulus a distortion polynomial is locally indistinguishable from a
/// scale — which is exactly why three frames read 0.984/1.000/1.004 and the
/// probe frames read ≈1.032. So the sidecar's stored geometry lives in the
/// PLAIN frame (measures 0.998 with the profile off), Lightroom rasterises
/// the BRUSH mask before its lens correction, and the export shows it warped
/// by a per-lens per-focal polynomial. ~~this engine does not model (no
/// `.lcp` parser; `crs:LensProfileEnable` is never read…)~~ **Both of those
/// parentheses expired on 2026-08-20 (R29 Batch-3) and are corrected below.**
///
/// **RULED 2026-08-19 (user): `1.0`** — render the geometry the sidecar
/// actually stores (the plain frame, measured 0.998 with the profile off)
/// and leave Adobe's warp to a model rather than to one frame's polynomial
/// sample. Strictly better than 1.032 on every frame measured in Batches 8
/// and 10. The `k` plumbing below is kept; at 1.0 every affine below is the
/// identity.
///
/// # What R29 Batch-3 changed, and what it did NOT
///
/// The two things this comment used to say the engine lacked, it now has:
/// [`crate::lcp`] parses Adobe's `.lcp` profiles, and
/// [`lens_profile_enabled`] reads `crs:LensProfileEnable`. The warp itself is
/// solved into [`crate::recipe::LensProfile::mask_warp`] from either the
/// in-camera knots or an `.lcp`, with
/// [`crate::recipe::MaskWarpSource`] naming which — or which of five refusals
/// applies.
///
/// **This constant stays `1.0`, and now for a POSITIVE reason rather than for
/// want of a model.** This boundary represents the STORED sidecar frame, and
/// therefore preserves the coordinates Lightroom wrote. D2 measured two
/// distinct render laws. RADIAL uses point transport about the full-raw centre.
/// LINEAR stores corrected-frame handles: correction ON evaluates its straight
/// gradient in that frame, while correction OFF maps only Zero/Full forward and
/// rebuilds one straight raw-frame gradient. Neither is an XMP-coordinate
/// conversion. Brush dabs remain pre-correction identity.
///
/// The render owns those frame operations. RADIAL asks each pre-geometry sample
/// at `m_lr⁻¹(T_engine(p))`. Active LINEAR asks it at `T_engine(p)` only;
/// inactive LINEAR maps its two handles once through `D_fwd`. This constant
/// preserves the stored parameters for all three. The 105 mm observation still
/// rejects moving RADIAL with the pixel field blindly: pixels move +87.5 px at
/// r≈3250 while the mask similarity is 0.99956.
///
/// These are deliberate RENDER-BEHAVIOUR changes introduced for the v1.0.0
/// release. See the mask-warp block header in `render.rs` for the frame table,
/// measured magnitudes and regression pins.
/// Nothing about it reaches this constant, which is the point.
pub(super) const LR_MASK_FRAME_SCALE: f64 = 1.0;

/// The frame every normalised `crs:` coordinate — mask box AND crop rectangle
/// — is measured against: the SOURCE frame's pixel size, plus the turn that
/// carries it into the frame this engine displays.
///
/// **`W, H` are the UN-ROTATED SOURCE frame** = the DNG/RAW `DefaultCropSize`,
/// NOT the raw `ImageWidth/Length` and NOT the exported pixel dimensions
/// (`PROBE2-VERDICT.md` §3.3: decoding in the raw frame splits `k_A` and `k_B`
/// by 0.35 % where the source frame holds them to 0.02 %). Two R27
/// measurements pin the two ways "exported dimensions" was wrong:
///
/// * **Cropped** (`P5-cropped-mask-frame.md` §1, HIGH). `PROBE4-FINAL.md` §4
///   opens with "`W, H` = the exported pixel dimensions ( = DNG
///   `DefaultCropSize`)". Those two readings coincide only while
///   `HasCrop="False"`; they diverge the moment a crop exists, and
///   `DefaultCropSize` is the correct one — the mask is laid out on the
///   UNCROPPED frame and the crop is a window onto it (22 of 23 shared masks
///   byte-identical across a crop change, a matched-filter limit of 0.09 % on
///   any crop-frame coupling against a 6.1 DN positive control). Feeding the
///   exported dimensions of a cropped render into the decode displaces
///   `P32_16.9.JPG`'s five radials by **834–1384 px**.
/// * **Portrait** (`P1-portrait-mask-frame.md` §1, HIGH). For a
///   `tiff:Orientation` 5–8 capture the source frame is the un-rotated SENSOR
///   array (9504 × 6336), and the export is already upright with **no**
///   orientation tag and no `tiff:` at all — so reading the JPEG's own
///   dimensions as the mask frame, "the natural thing to do, and what a
///   decoder does by default", is exactly the defect. 7/7 files pick their
///   true frame by `dSS`, and a pure radial declaring +1.6 EV reads as
///   +1.65 EV under the sensor frame and **−1.98 EV** (wrong sign) under the
///   display one.
///
/// Only the RATIO enters the ellipse projection — the decode multiplies the
/// stored half-extents by `W` and `H` to reach pixels and divides by them
/// again to reach the engine's own normalised frame, so `W` and `H` cancel and
/// `s = W/H` is all that survives. The SIZE is kept anyway because the writer
/// declares it (`tiff:ImageWidth/ImageLength`), which is what lets a document
/// we authored be re-imported in the frame it was written in.
///
/// [`turn`](Self::turn) is the map SOURCE → DISPLAY: the capture's EXIF
/// orientation composed with the photographer's own quarter turns
/// ([`crate::render::compose_orientation`]). The projection decodes in the
/// source frame and then moves the whole recipe through
/// [`crate::render::orient_recipe_coords`], which is the algebra this build
/// already owns for the `coord_era` migration — one turn, every geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameAspect {
    pub(super) w: f64,
    pub(super) h: f64,
    pub(super) turn: rawler::Orientation,
}

impl FrameAspect {
    /// From a SOURCE pixel size whose display frame is the same frame (a
    /// landscape capture, a baked image whose pixels are already upright).
    /// `None` for anything that is not a positive, finite rectangle — a zero
    /// dimension would make the projection singular.
    pub fn from_size(w: f64, h: f64) -> Option<Self> {
        FrameAspect::from_size_turned(w, h, rawler::Orientation::Normal)
    }

    /// [`from_size`](Self::from_size) for a capture whose display frame is the
    /// source frame TURNED — `turn` is the composed orientation
    /// ([`crate::render::compose_orientation`]), i.e. the same state
    /// `orient_recipe_coords` takes.
    pub fn from_size_turned(w: f64, h: f64, turn: rawler::Orientation) -> Option<Self> {
        (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0)
            .then_some(FrameAspect { w, h, turn })
    }

    /// `s = W/H` in the SOURCE frame — the one number the ellipse projection
    /// needs.
    pub(super) fn aspect(&self) -> f64 {
        self.w / self.h
    }

    /// The source → display turn. `Normal` when the two frames coincide.
    pub(super) fn turn(&self) -> rawler::Orientation {
        self.turn
    }

    /// The SAME frame with the turn already folded in: the display rectangle,
    /// declaring no turn of its own. This is what a document that will NOT
    /// carry a `tiff:Orientation` must be projected in — geometry and
    /// declaration have to agree, and a document that declares nothing
    /// declares the frame its own pixels are in.
    pub(super) fn displayed(&self) -> Self {
        let (w, h) = if crate::decode::orientation_transposes(self.turn) {
            (self.h, self.w)
        } else {
            (self.w, self.h)
        };
        FrameAspect { w, h, turn: rawler::Orientation::Normal }
    }

    /// The frame a document DECLARES, from `tiff:ImageWidth` /
    /// `tiff:ImageLength` / `tiff:Orientation`.
    ///
    /// Read off a scope of their OWN, not the crs one: these are `tiff:`
    /// properties, and while they sit on the same `rdf:Description` as the crs
    /// settings in every Lightroom sidecar seen here, nothing makes that
    /// structural — an XMP packet may carry one Description per namespace.
    ///
    /// **They must come from ONE Description, though** (R28 Batch-5 5d, F4
    /// symptom D). The three used to be three independent first-occurrence
    /// searches over the whole document, so a width from one element could be
    /// paired with a length from another and an orientation from a third — a
    /// frame no element in the file actually declares, handed to the mask and
    /// crop decoders as the coordinate system to fold pixel geometry with.
    /// [`FrameScope::resolve`] picks the first `rdf:Description` that declares
    /// BOTH dimensions and all three are read from THAT scope — a scope that is
    /// a TYPE since R29-2, so a caller cannot skip the narrowing by forgetting
    /// it instead of by meaning to. Its two whole-document fallbacks (a
    /// document with no `rdf:Description` at all; one whose Descriptions never
    /// carry both dimensions together) are intended behaviour and are spelled
    /// out there.
    ///
    /// The declared pair is taken VERBATIM as the source frame, including for
    /// the transposing orientations — `F3-REPORT.md`'s public census is 72/72
    /// that a sidecar's `tiff:ImageWidth/ImageLength` are always the sensor
    /// (landscape) frame, and `P1-portrait-mask-frame.md` §3 confirms it on the
    /// user's own library (`P42.xmp`: `tiff:Orientation="8"` beside the
    /// ARW's `DefaultCropSize = (9504, 6336)`, against a 6336 × 9504 export).
    /// The pre-R27 comment here called that swap "unmeasured"; it is measured
    /// now, and the swap is the DECODER's job (see [`turn`](Self::turn)), not a
    /// reinterpretation of the declaration.
    ///
    /// A missing `tiff:Orientation` reads as `Normal`, which is the same thing
    /// EXIF itself means by an absent tag.
    pub(super) fn from_xmp(doc: &str) -> Option<Self> {
        let span = FrameScope::resolve(doc);
        FrameAspect::from_size_turned(
            span.declared_number("tiff:ImageWidth")?,
            span.declared_number("tiff:ImageLength")?,
            span.declared_orientation().unwrap_or(rawler::Orientation::Normal),
        )
    }
}

/// **The scope a `tiff:` frame read is allowed to see — as a TYPE** (R29-2,
/// finishing R28 Batch-5 5d on the namespaces 5d did not reach).
///
/// 5d put the `crs:` side's scope in the signature ([`Tag`] / [`Scope`]) and
/// left the `tiff:` frame family on the old shape: a free
/// `declared_number(doc: &str, name: &str)` whose `doc` meant "the ONE
/// Description this frame is read from" at one call site and "the whole
/// document" at the next, kept straight only by the CONVENTION that every
/// caller ran the narrowing scan (then `frame_description`) first. That is
/// exactly the per-call-site convention 5d removed next door — and the property
/// it guards is the coordinate system every mask and crop decode folds pixel
/// geometry with ([`lr_to_engine`]).
///
/// So the scope is a type here too. A `FrameScope` is produced by
/// [`resolve`](Self::resolve) and nothing else, so "which span is this?" is
/// answered once, by the resolver, instead of separately in each caller's head.
///
/// A dedicated newtype rather than a second meaning bolted onto 5d's [`Scope`]:
/// `Scope`'s reads are `crs:`-anchored ([`CrsSource`]) and its `new` takes any
/// text, so teaching it `tiff:` would hand all 107 existing `crs:` call sites
/// the power to ask a frame question of an unresolved document — the same
/// convention back again, one type wider.
#[derive(Clone, Copy)]
pub(super) struct FrameScope<'a>(&'a str);

impl<'a> FrameScope<'a> {
    /// The ONE `rdf:Description` a document's `tiff:` frame is read from — the
    /// first one that declares both `tiff:ImageWidth` and `tiff:ImageLength`
    /// (R28 Batch-5 5d, F4 symptom D).
    ///
    /// The span runs from that element's `<` to the end of its BODY (the close
    /// tag carries no attributes, so it is not needed), which is what keeps
    /// BOTH XMP spellings working: the attribute form Lightroom writes lives on
    /// the start tag, and the property-element form lives in the body.
    ///
    /// **Two fallbacks to the WHOLE document, both intended, both pinned by the
    /// R29-2 fixtures at the bottom of this file:**
    ///
    /// * there is no `rdf:Description` in the text at all. A bare fragment
    ///   cannot mix two elements' declarations, so the narrowing has nothing to
    ///   protect there, and refusing would break every reader that hands this a
    ///   snippet — fixtures in this module do exactly that.
    /// * Descriptions are present, but no single one carries both dimensions.
    ///   That is the shape the pre-5d code read; the aspect is disclosed as
    ///   degraded downstream either way, and inventing a refusal here would drop
    ///   the frame for files nobody has measured. The residue is real and named:
    ///   in THAT document the width and the length may still come from different
    ///   elements — symptom D is removed only for documents where some element
    ///   declares both.
    pub(super) fn resolve(doc: &'a str) -> Self {
        Self::resolve_with_start(doc).0
    }

    /// [`resolve`] plus WHERE it resolved to: the `<` of the Description it
    /// narrowed to, or `None` for either whole-document fallback. The merge
    /// asks, because a declaration it corrects has to land inside the span
    /// this reader will search, or the engine's own round trip loses it.
    pub(super) fn resolve_with_start(doc: &'a str) -> (Self, Option<usize>) {
        let mut from = 0;
        while let Some((start, gt, self_closing)) = next_xml_tag(doc, from) {
            from = gt + 1;
            let tag = &doc[start..=gt];
            if tag.starts_with("</") || tag_name(tag) != "rdf:Description" {
                continue;
            }
            let end = if self_closing {
                gt + 1
            } else {
                match find_matching_close(doc, gt + 1) {
                    // `find_matching_close` returns the `<` of the close tag; the
                    // close itself carries no attributes, so the body is enough.
                    Some(close) => close,
                    None => continue,
                }
            };
            let span = FrameScope(&doc[start..end]);
            if span.declared_number("tiff:ImageWidth").is_some()
                && span.declared_number("tiff:ImageLength").is_some()
            {
                return (span, Some(start));
            }
        }
        (FrameScope(doc), None)
    }

    /// The number THIS scope declares for `name`, in either XMP spelling —
    /// `name="9504"` (the attribute form Lightroom writes) or
    /// `<name>9504</name>` (the element form the same property is legal in).
    ///
    /// Scanned with [`find_outside_constructs`], so a comment quoting the
    /// property cannot answer for the scope — the rule this module already
    /// enforces for the merge splice. A hit whose next non-space character is
    /// neither `=` nor `>` (i.e. the needle was the prefix of a LONGER name)
    /// gives up rather than searching on: giving up costs the caller its frame,
    /// which is a disclosed, degraded path, where searching on could answer
    /// from an unrelated property.
    pub(super) fn declared_number(self, name: &str) -> Option<f64> {
        let doc = self.0;
        let at = find_outside_constructs(doc, name)?;
        let rest = doc[at + name.len()..].trim_start();
        let text = match rest.strip_prefix('=') {
            Some(v) => {
                let v = v.trim_start();
                let quote = v.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                v[quote.len_utf8()..].split(quote).next()?
            }
            None => rest.strip_prefix('>')?.split('<').next()?,
        };
        xml_unescape(text)
            .trim()
            .trim_start_matches('+')
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    }

    /// The `tiff:Orientation` THIS scope declares, or `None` when it declares
    /// none — the ONE parser of that property, so the frame read above and the
    /// import law below cannot disagree about a value.
    ///
    /// **`None` and `Some(Normal)` are different answers**, and both callers
    /// need the difference: a document that declares no orientation says
    /// nothing about which way the photograph is up (and the photograph's own
    /// EXIF decides), while one that declares `1` says it is upright — which,
    /// over a capture whose EXIF says otherwise, is a rotation the
    /// photographer made in Lightroom. A number outside EXIF's own 1..=8
    /// domain reads as no declaration rather than as `Unknown`, which is what
    /// the `filter` in `from_xmp` has always done.
    pub(super) fn declared_orientation(self) -> Option<rawler::Orientation> {
        self.declared_number("tiff:Orientation")
            .filter(|v| (1.0..=8.0).contains(v))
            .map(|v| rawler::Orientation::from_u16(v as u16))
    }
}

/// The `tiff:Orientation` a whole document declares, through the same scope
/// resolution [`FrameAspect::from_xmp`] uses — `None` when it declares none.
///
/// Separate from `from_xmp` because the two questions come apart in the field:
/// 154 of the 174 sidecars in the reference library declare an orientation and
/// NO `tiff:ImageWidth`/`tiff:ImageLength` at all, so a reader that could only
/// learn the turn along with the rectangle would learn it for 18 of them.
pub(super) fn declared_orientation(doc: &str) -> Option<rawler::Orientation> {
    FrameScope::resolve(doc).declared_orientation()
}

/// What a sidecar's `tiff:Orientation` means for the photograph beside it: the
/// frame its `crs:` coordinates must be carried INTO, and the photographer's
/// turn that makes this engine's render DELIVER that frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HonouredTurn {
    /// The source -> display turn [`crate::render::orient_recipe_coords`]
    /// moves the imported geometry by.
    pub(super) turn: rawler::Orientation,
    /// [`EditRecipe::quarter_turns`], chosen so that
    /// `compose_orientation(the RAW's EXIF, quarter_turns) == turn`.
    pub(super) quarter_turns: u8,
    /// The declared state that could NOT be delivered, for the disclosure.
    /// `Some` only for a mirror ([`crate::render::quarter_turns_between`]).
    pub(super) refused: Option<rawler::Orientation>,
}

/// [`crate::render::quarter_turns_between`] — the sidecar orientation law —
/// applied to one document / photograph pair. The law itself, its measurement
/// and its sign convention live on that function; this is the import side's
/// half of the citation.
///
/// Three arms, and the first two are today's behaviour byte for byte:
///
/// * the document declares no orientation ⇒ it says nothing about which way
///   the photograph is up, so neither does the recipe;
/// * there is no photograph to compose against — a baked source, an unreadable
///   RAW ⇒ the declaration is the only frame information there is and is taken
///   at face value, which is what every build before this one did with it;
/// * both are known ⇒ the law. `Some(k)` is the turn that makes the delivered
///   frame equal Lightroom's; `None` is a MIRROR, refused whole.
///
/// **A refused tag is treated as no tag**: the photograph's own EXIF decides
/// the frame, so the geometry still lands on the part of the picture Lightroom
/// put it on and only the handedness is lost. Honouring half of it — turning
/// the coordinates into a frame the render will never produce — would move
/// every mask in the file instead.
pub(super) fn honour_declared_orientation(
    declared: Option<rawler::Orientation>,
    exif: Option<rawler::Orientation>,
) -> HonouredTurn {
    let plain = |turn| HonouredTurn { turn, quarter_turns: 0, refused: None };
    let Some(want) = declared else { return plain(rawler::Orientation::Normal) };
    let Some(exif) = exif else { return plain(want) };
    match crate::render::quarter_turns_between(exif, want) {
        Some(quarter_turns) => HonouredTurn { turn: want, quarter_turns, refused: None },
        None => HonouredTurn { turn: exif, quarter_turns: 0, refused: Some(want) },
    }
}

/// The frame an orientation-only document's geometry was written in — the
/// photograph's own rectangle under the honoured turn — or `None` without
/// a readable photograph. `merge_frame`'s Keep and Orientation arms fold a
/// recipe through exactly this frame when the base declares no rectangle,
/// so every reader of such a document decodes in it: the recipe reader
/// (`xmp_to_recipe_clamped_impl`) and the photo-aware disclosure doors
/// ([`crop_import_note_for_photo`], [`import_losses_for_photo`]).
pub(super) fn frame_fallback(
    source: Option<((usize, usize), rawler::Orientation)>,
    turn: &HonouredTurn,
) -> Option<FrameAspect> {
    source.and_then(|((w, h), _)| FrameAspect::from_size_turned(w as f64, h as f64, turn.turn))
}

/// [`frame_fallback`] from a document and the photograph beside it: the
/// document's declared orientation composed with the RAW's own frame, the
/// way the recipe reader composes them. `None` when the document declares
/// no orientation (nothing was folded) or the photograph is not a readable
/// RAW (nothing to compose against — the declaration stands alone).
pub(super) fn photo_frame_fallback(xmp: &str, photo: Option<&std::path::Path>) -> Option<FrameAspect> {
    let declared = declared_orientation(xmp)?;
    let photo = photo.filter(|p| crate::decode::is_raw(p))?;
    let source = crate::pipeline::source_frame_memo(photo)?;
    let turn = honour_declared_orientation(Some(declared), Some(source.1));
    frame_fallback(Some(source), &turn)
}
