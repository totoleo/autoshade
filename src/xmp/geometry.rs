//! Mask geometry between Lightroom and the engine: the radial ellipse decode, the crop rectangle both ways, and the crop import notes.

use super::*;

/// The five numbers Lightroom stores for one `Mask/CircularGradient`, in its
/// own normalised frame. NOT a bounding box — see [`lr_to_engine`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LrRadial {
    pub(super) top: f64,
    pub(super) left: f64,
    pub(super) bottom: f64,
    pub(super) right: f64,
    pub(super) angle_deg: f64,
}

/// The five numbers THIS engine stores for the same ellipse
/// ([`MaskGeometry::Radial`]): an axis-aligned box in the engine's normalised
/// frame plus a rotation applied in that frame. Same shape as [`LrRadial`],
/// deliberately a different type — the two are not interchangeable and mixing
/// them up is exactly the defect this batch closed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct EngineRadial {
    pub(super) top: f64,
    pub(super) left: f64,
    pub(super) bottom: f64,
    pub(super) right: f64,
    pub(super) angle_deg: f64,
}

/// What [`lr_to_engine`] could make of one stored radial.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum RadialDecode {
    /// Decoded whole: shape, tilt and position all carried.
    Exact(EngineRadial),
    /// The document declares no frame ([`FrameAspect::from_xmp`]), so a
    /// non-zero `crs:Angle` cannot be decoded — the rotation is a PIXEL-frame
    /// tilt and turning it into this engine's normalised-frame one needs the
    /// aspect. The axis-aligned reading rides through (with the frame affine,
    /// which needs no aspect) and the caller raises the rotation disclosure,
    /// exactly as every build before v0.32.0 did for every rotated radial.
    Unrotated(EngineRadial),
    /// The corners do not describe an ellipse at all — a DEGENERATE fold, one
    /// of whose semi-axes is zero or not a number. A zero-width ellipse is not
    /// an ellipse, and the renderer's `max(1e-4)` would draw it as a hairline.
    ///
    /// A NEGATIVE semi-axis is NOT this: it used to be refused here, under an
    /// `a > 0 ∧ b > 0` guard, and the me6-2026-09 pack refuted the refusal.
    /// Group D's nine exports hand Lightroom a plain box (Left < Right,
    /// Top < Bottom) with `crs:Angle="30"`, which folds to `b = −0.020096`;
    /// Lightroom renders the ellipse of |b| — imported that way, the nine match
    /// Lightroom's own α to a pooled rms of 0.0069, the same order as every
    /// other family in the pack, with the α = ½ contour +6.3 / +8.9 / +12.8 px
    /// out at Feather 25 / 50 / 75 on a 1248 px scale. Refused, they rendered
    /// nothing at all (pooled rms 0.3931). The ellipse metric is
    /// `(u/a)² + (v/b)²` and cannot see the sign; this engine's own
    /// `mask_weight` already takes `.abs()` of both semi-axes for that reason,
    /// so the refusal was the only thing standing between a readable file and
    /// the mask it describes.
    Refused,
}

/// The 2×2 SVD in closed form: `m = R(θᵤ)·diag(σ₁, σ₂)·R(θᵥ)ᵀ`, returning
/// `(σ₁, σ₂, θᵤ)` with `θᵤ` in radians and `σ₁ ≥ |σ₂|`.
///
/// `σ₂` comes out NEGATIVE when `det m < 0`; callers take `|σ₂|` and keep
/// `R(θᵤ)` as the rotation. That is not a shortcut — `diag(σ₁, −|σ₂|)` is
/// `diag(σ₁, |σ₂|)` composed with a reflection, and a reflection maps the unit
/// circle to itself, so the ELLIPSE (which is all this is used for) is
/// unchanged. It is also what `ANGLE-MODEL.md` §6.1's "force `det U > 0`"
/// asks for, reached without a branch.
///
/// `m` is `[m00, m01, m10, m11]`.
pub(super) fn svd2(m: [f64; 4]) -> (f64, f64, f64) {
    let (e, f) = ((m[0] + m[3]) / 2.0, (m[0] - m[3]) / 2.0);
    let (g, h) = ((m[2] + m[1]) / 2.0, (m[2] - m[1]) / 2.0);
    let (q, r) = (e.hypot(h), f.hypot(g));
    (q + r, q - r, (h.atan2(e) + g.atan2(f)) / 2.0)
}

/// Fold an ellipse orientation into Lightroom's own canonical window.
///
/// An ellipse's orientation is defined mod 180°, so `(box, angle)` is redundant
/// twice over and Lightroom resolves it by keeping `|θ| ≤ 45°` and letting the
/// `a > b` / `a < b` distinction carry the other quadrant: all 195 radials in
/// the user's library sit in `[−43.945, +44.793]`, zero rows outside ±45°
/// (`BBOX-DECODE.md` §2.3), and LRTimelapse's author has the flip on the record
/// ("Lightroom flips from +45 to −45"). Returns the folded angle in degrees and
/// whether the semi-axes must be SWAPPED to go with it.
fn canonical_lr_angle(deg: f64) -> (f64, bool) {
    let mut d = deg.rem_euclid(180.0);
    if d > 90.0 {
        d -= 180.0;
    }
    if d.abs() > 45.0 { (d - 90.0 * d.signum(), true) } else { (d, false) }
}

/// Lightroom's stored radial → this engine's `MaskGeometry::Radial` geometry.
///
/// **`crs:Top/Left/Bottom/Right` is not a bounding box.** It is the pair of
/// ROTATED CORNERS of the ellipse's own box, written in the frame's PIXEL
/// coordinates (`BBOX-DECODE.md` §1):
///
/// ```text
///     (Left, Top)     = centre + R(θ)·(−a, −b)
///     (Right, Bottom) = centre + R(θ)·(+a, +b)
/// ```
///
/// so the decode is
///
/// ```text
///     X = (R−L)/2·W        Y = (B−T)/2·H          SIGNED, never abs()
///     a =  X·cos θ + Y·sin θ     b = −X·sin θ + Y·cos θ
/// ```
///
/// Read naively — `rx = (R−L)/2`, which is what every build up to v0.31.2 did
/// and what every other implementation in the searchable world still does —
/// the axis ratio is wrong by a median factor of 1.84 over the user's rotated
/// components, p90 4.86, max 40.7; it frequently assigns the MAJOR axis to the
/// wrong axis; and 16 of 195 components decode to a NEGATIVE semi-axis, i.e.
/// they are not readable at all. The model is not a fit: it makes a sign
/// prediction with no free parameters (`Left > Right` forces `Angle > 0`,
/// `Top > Bottom` forces `Angle < 0`, both at once is impossible) which the
/// library confirms 16/16 at p = 2.5 × 10⁻⁵, and the two rendered subjects
/// `P24` (8.3 : 1 at +24.35°) and `P22` (1 : 2 at +29.51°, decoded
/// tilt −60.486° against a measured −60.5°) land on it at the pixel
/// (`PROBE2-VERDICT.md` §1, §5).
///
/// The `(a, b)` here are computed in units of the frame HEIGHT rather than in
/// pixels — `W` and `H` cancel out of the whole projection and only `s = W/H`
/// survives (see [`FrameAspect`]). The `a > 0 ∧ b > 0` guard is unaffected: `W`
/// and `H` are positive, so scaling cannot change a sign.
///
/// [`MaskGeometry::Radial`]: crate::recipe::MaskGeometry::Radial
pub(super) fn lr_to_engine(lr: LrRadial, frame: Option<FrameAspect>) -> RadialDecode {
    let k = LR_MASK_FRAME_SCALE;
    let (ncx, ncy) = ((lr.left + lr.right) / 2.0, (lr.top + lr.bottom) / 2.0);
    let (xn, yn) = ((lr.right - lr.left) / 2.0, (lr.bottom - lr.top) / 2.0);
    // The centre moves with the frame, not just the axes — this is the half
    // `PROBE4-FINAL.md` settled and the half a "scale the semi-axes" reading
    // gets wrong by 88 px at a frame corner. It needs no aspect.
    let boxed = |rx: f64, ry: f64, angle_deg: f64| EngineRadial {
        left: k * ncx - (k - 1.0) / 2.0 - rx,
        right: k * ncx - (k - 1.0) / 2.0 + rx,
        top: k * ncy - (k - 1.0) / 2.0 - ry,
        bottom: k * ncy - (k - 1.0) / 2.0 + ry,
        angle_deg,
    };
    // An UNROTATED radial decodes identically under both readings (115 of the
    // library's 195 components), and the identity needs no aspect and no SVD —
    // taking it verbatim keeps those masks bit-stable instead of routing them
    // through a numerical fold that can only lose digits.
    //
    // The guard still applies, and at `θ = 0` the fold IS the box, so here it
    // reads as `R ≠ L ∧ B ≠ T`: a degenerate box is refused — a zero-width
    // ellipse is not an ellipse, where the renderer's `max(1e-4)` used to draw
    // it as a hairline. An INVERTED corner pair is not degenerate and is no
    // longer refused; see [`RadialDecode::Refused`] for the group-D
    // measurement, and note that this arm has to be the `θ → 0` limit of the
    // folded one below — one law written twice would be two laws. The 115
    // unrotated components in the user's library all have `xn > 0 ∧ yn > 0`,
    // so `abs()` is the identity on every one of them and they stay
    // bit-stable; an inverted pair now imports as the ellipse it describes and
    // re-exports with its corners sorted, which is what the no-frame arm below
    // has always done with one.
    if lr.angle_deg == 0.0 {
        return if xn != 0.0 && yn != 0.0 {
            RadialDecode::Exact(boxed(k * xn.abs(), k * yn.abs(), 0.0))
        } else {
            RadialDecode::Refused
        };
    }
    let Some(s) = frame.map(|f| f.aspect()) else {
        // No declared frame: the naive box, as before v0.32.0, with the
        // rotation disclosed rather than silently applied or silently dropped.
        //
        // `abs()` here and nowhere else. A box on this path may legitimately
        // carry `Left > Right` (it is rotated — that is why we are here), and
        // the signed reading of it is exactly what needs the aspect we do not
        // have. So the axis-aligned fallback takes the magnitudes, which is
        // what `mask_weight` would have done with them anyway, and the
        // disclosure says the rotation did not arrive. The cost is byte
        // fidelity on re-export for that one shape: an inverted pair comes back
        // sorted. It comes back describing what this build renders, which the
        // alternative does not.
        return RadialDecode::Unrotated(boxed(k * xn.abs(), k * yn.abs(), 0.0));
    };
    let (sin, cos) = lr.angle_deg.to_radians().sin_cos();
    let (a, b) = (xn * s * cos + yn * sin, -xn * s * sin + yn * cos);
    // DEGENERATE is refused; NEGATIVE is not degenerate. See
    // [`RadialDecode::Refused`] for the group-D measurement that separated the
    // two: an ellipse is `(u/a)² + (v/b)²`, so a negative fold names the same
    // curve as its magnitude, and Lightroom draws that curve. `is_finite`
    // stays because the old `a > 0 && b > 0` also caught a NaN corner, and a
    // NaN semi-axis would reach the renderer as a NaN weight.
    if !(a.is_finite() && b.is_finite()) || a == 0.0 || b == 0.0 {
        return RadialDecode::Refused;
    }
    // Into the engine's normalised frame: `rx = k·a/W`, `ry = k·b/H`, which in
    // height units is `k·a/s` and `k·b`.
    let (rx, ry) = (k * a / s, k * b);
    // …and fold the PIXEL-frame rotation into the engine's NORMALISED-frame one
    // (`ANGLE-MODEL.md` §6.1). The two differ by up to 11.2° of rendered tilt
    // over the library's `|angle| ≤ 44°` range (§3.5), measured 28.554° against
    // a normalised-frame prediction of 19.692° on `P19` (§3.2).
    let m = [rx * cos, -ry * sin / s, rx * s * sin, ry * cos];
    let (s1, s2, tu) = svd2(m);
    RadialDecode::Exact(boxed(s1.abs(), s2.abs(), tu.to_degrees()))
}

/// This engine's radial geometry → Lightroom's stored corners. The exact
/// inverse of [`lr_to_engine`]; `R(θ)` is orthogonal, so the round trip is
/// algebraically exact and the two legal corner arrangements Lightroom writes
/// (`Left < Right` and `Left > Right`) both come back byte-stable.
///
/// `None` for the frame means the caller could not learn the aspect, and a
/// non-zero engine angle then cannot be projected: the unrotated ellipse is
/// returned with the angle it could not write, for the caller to disclose.
/// The frame AFFINE is applied either way — it needs no aspect, and leaving it
/// off would make the writer the inverse of nothing.
pub(super) fn engine_to_lr(e: EngineRadial, frame: Option<FrameAspect>) -> (LrRadial, Option<f64>) {
    let k = LR_MASK_FRAME_SCALE;
    let (cx, cy) = ((e.left + e.right) / 2.0, (e.top + e.bottom) / 2.0);
    let (rx, ry) = (((e.right - e.left) / 2.0).abs(), ((e.bottom - e.top) / 2.0).abs());
    let (ncx, ncy) = ((cx + (k - 1.0) / 2.0) / k, (cy + (k - 1.0) / 2.0) / k);
    let corners = |xn: f64, yn: f64, angle_deg: f64| LrRadial {
        left: ncx - xn,
        right: ncx + xn,
        top: ncy - yn,
        bottom: ncy + yn,
        angle_deg,
    };
    let unrotated = |withheld: Option<f64>| (corners(rx / k, ry / k, 0.0), withheld);
    if e.angle_deg == 0.0 {
        return unrotated(None);
    }
    let Some(s) = frame.map(|f| f.aspect()) else {
        return unrotated(Some(e.angle_deg));
    };
    // `diag(s, 1)·R(angle)·diag(rx, ry)` — the ellipse carried into the
    // isotropic (pixel-proportional) frame, whose SVD reads off the pixel tilt
    // and the pixel semi-axes in units of the frame height.
    let (sin, cos) = e.angle_deg.to_radians().sin_cos();
    let (s1, s2, tu) = svd2([s * cos * rx, -s * sin * ry, sin * rx, cos * ry]);
    let (mut a, mut b) = (s1.abs(), s2.abs());
    let (deg, swap) = canonical_lr_angle(tu.to_degrees());
    if swap {
        std::mem::swap(&mut a, &mut b);
    }
    // Undo the frame affine on the axes, then re-encode the corners. Do NOT
    // sort or clamp the result: when `tan θ > a/b` this legitimately emits
    // `Left > Right`, which is byte-for-byte what Lightroom itself writes
    // (6/6 such rows in the library carry `Angle > 0`, as the model requires),
    // and normalising the box to min/max destroys the mask.
    let (a, b) = (a / k, b / k);
    let (sin, cos) = deg.to_radians().sin_cos();
    (corners((a * cos - b * sin) / s, a * sin + b * cos, deg), None)
}

/// The five numbers Lightroom stores for the CROP — `crs:Crop{Left,Top,Right,
/// Bottom}` plus `crs:CropAngle`. NOT an axis-aligned rectangle: see
/// [`lr_to_engine_crop`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LrCrop {
    pub(super) left: f64,
    pub(super) top: f64,
    pub(super) right: f64,
    pub(super) bottom: f64,
    pub(super) angle_deg: f64,
}

/// What [`lr_to_engine_crop`] could make of one stored crop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum CropDecode {
    /// Decoded into this engine's composition. `crop` is `None` for the
    /// rectangle that IS the whole straightened frame (the straighten-only
    /// carrier, which the writer emits and this collapses back).
    /// `overshoot_frac` is how far outside the straightened frame the
    /// rectangle reached before being clamped, in units of the SOURCE frame's
    /// height — the one length every quantity in [`lr_to_engine_crop`] is
    /// measured in, so `overshoot_frac × tiff:ImageLength` is the overshoot
    /// in pixels whichever edge it was on. `0.0` when the conversion was
    /// exact; this is the only lossy edge of the whole conversion (see §"the
    /// one inexactness" below).
    Read { crop: Option<Crop>, straighten_deg: f64, overshoot_frac: f64 },
    /// Tilted, and the document declares no frame ([`FrameAspect::from_xmp`]):
    /// the rectangle's own side lengths cannot be recovered without `W/H`, so
    /// the crop is dropped and the straighten — which needs no aspect — rides
    /// alone. Disclosed, never silent.
    NoFrame { straighten_deg: f64 },
    /// The corners do not describe a rectangle at the declared angle (the
    /// `p > 0 ∧ q > 0` guard), or they leave `[0,1]`. Real Lightroom data
    /// satisfies both; a document that does not is malformed, and cropping to
    /// it would deliver a frame the file does not describe. The tilt is a
    /// separate attribute and is still readable, so it rides — which is what
    /// every build before R27 did with an unreadable rectangle.
    Refused { straighten_deg: f64 },
}

impl CropDecode {
    /// The rectangle this engine will apply, or `None` — no crop, an
    /// unreadable one, or the straighten-only carrier.
    pub(super) fn crop(self) -> Option<Crop> {
        match self {
            CropDecode::Read { crop, .. } => crop,
            CropDecode::NoFrame { .. } | CropDecode::Refused { .. } => None,
        }
    }

    /// The straighten this engine will apply, in its own clockwise-positive
    /// degrees. Readable on every arm: it needs no aspect and no rectangle.
    pub(super) fn straighten_deg(self) -> f64 {
        match self {
            CropDecode::Read { straighten_deg, .. }
            | CropDecode::NoFrame { straighten_deg }
            | CropDecode::Refused { straighten_deg } => straighten_deg,
        }
    }
}

/// The straightened frame's size in units of the SOURCE frame's height — the
/// rectangle [`crate::render::rotate_straighten`] leaves behind, which is the
/// frame this engine's [`Crop`] fractions are measured against.
///
/// `inscribed_dims` is homogeneous of degree 1 in `(w, h)`, so evaluating it at
/// `(s, 1)` gives the same FRACTIONS as evaluating it at `(W, H)` — and the
/// renderer's own call is the one definition, called here rather than copied.
pub(super) fn inscribed_norm(s: f64, straighten_deg: f64) -> (f64, f64) {
    let (w, h) = crate::render::inscribed_dims(s as f32, 1.0, straighten_deg as f32);
    (w as f64, h as f64)
}

/// Lightroom's stored crop → this engine's `(Crop, straighten_deg)`.
///
/// **`crs:Crop{Left,Top,Right,Bottom}` is not an axis-aligned rectangle.** It
/// is the pair of opposite ROTATED CORNERS of the crop rectangle, written as
/// plain fractions of the un-rotated SOURCE frame — the IDENTICAL encoding
/// [`lr_to_engine`] decodes for `Mask/CircularGradient`, one family, two
/// consumers (`P3-cropangle-model.md` §1, HIGH):
///
/// ```text
///     (Left,  Top)    = centre + R(θ)·(−p, −q)
///     (Right, Bottom) = centre + R(θ)·(+p, +q)
///     X = (R−L)/2·W        Y = (B−T)/2·H          SIGNED, never abs()
///     p =  X·cos θ + Y·sin θ     q = −X·sin θ + Y·cos θ
///     W_out = round(2p)          H_out = round(2q)
/// ```
///
/// 7/7 photographs pixel-exact with ZERO free parameters, max residual 0.6 px,
/// across two signs of `CropAngle`, two `tiff:Orientation` states and two
/// source aspect ratios; seven rival models miss by 165–987 px, including the
/// naive AABB this build used until R27 (485 px, 0/7). There is **no `k`
/// magnification on the crop** — global best-fit scale 1.000006. (R27
/// Batch-10 dissolved the crop-vs-mask asymmetry this once stated: the mask
/// frame carries no `k` either — its 1.032 was one frame's lens-profile warp,
/// and `LR_MASK_FRAME_SCALE` is 1.0 by the 2026-08-19 ruling — so crop and
/// mask coordinates alike are plain fractions of the un-rotated source frame.)
///
/// **The sign** (`P3-cropangle-model.md` §4, HIGH, 34× margin on the weakest of
/// six photographs, 7257× on the best). `rot(source → export) = −CropAngle`:
/// Lightroom turns the CONTENT counter-clockwise by `+CropAngle`, i.e. the crop
/// BOX clockwise. [`crate::render::rotate_straighten`] turns the content
/// CLOCKWISE for a positive angle, so `straighten_deg = −CropAngle` — the
/// negation lives HERE, at the boundary, exactly like `LR_MASK_FRAME_SCALE`,
/// and the engine's own convention does not move. Until R27 the value rode 1:1
/// into a clockwise rotator, so every straightened import was tilted by
/// **2 × CropAngle** the wrong way (6.55° on the library's largest).
///
/// **The composition.** Lightroom has no auto-crop step: the crop rectangle is
/// given explicitly, in the source frame, and one resample produces the output.
/// This engine rotates first (auto-cropping to
/// [`crate::render::inscribed_dims`]) and then applies `Crop` as fractions of
/// THAT frame. The two are the same map wherever both can express the
/// rectangle, and this function is the conversion — `d = R(−θ)·(centre −
/// c_src)`, then `left = (d.x + W_i/2 − p)/W_i` (`P3-cropangle-model.md` §6.2's
/// "if the current order is kept").
///
/// **The one inexactness, measured not asserted.** The inscribed rectangle is
/// CENTRED, so a Lightroom crop pushed against the edge of the rotated frame
/// can reach outside it even when it is smaller. Over P3's seven measured
/// specimens the overshoot is 0.00 px, 0.16 px, 0.94 px, 0.95 px, 5.32 px,
/// 5.89 px and 46.77 px (0.85 % of one edge, `P44_1`) — so the conversion
/// is exact or sub-pixel on six of seven, and the seventh loses less than one
/// percent of one edge. The rectangle is CLAMPED (never sorted — see below)
/// and the amount is reported, because `EditRecipe::clamp` would otherwise do
/// the same clamp later, silently, at the first save.
///
/// **No ordering guard.** `Left > Right` is legal and reachable: under the
/// corner encoding it means `X < 0`, i.e. `tan θ > p/q`, which a 2:3 crop
/// straightened past +33.69° produces (`P3-cropangle-model.md` §6.3, and
/// `crs:CropAngle`'s own range is ±45°, F3 STRONG). The pre-R27 reader required
/// `left < right && top < bottom` and DISCARDED such a crop in silence. The
/// `[0,1]` half of the guard stays: both stored points are corners of a
/// rectangle Lightroom keeps inside the frame.
pub(super) fn lr_to_engine_crop(lr: LrCrop, frame: Option<FrameAspect>, ours: bool) -> CropDecode {
    // The engine's straighten is CLOCKWISE-positive; Lightroom's CropAngle is
    // the content's counter-clockwise turn. One negation, one place.
    let straighten_deg = -lr.angle_deg;
    let inside = |v: f64| (0.0..=1.0).contains(&v);
    if ![lr.left, lr.top, lr.right, lr.bottom].iter().copied().all(inside) {
        return CropDecode::Refused { straighten_deg };
    }
    // "The whole frame" with a hair of float slack: the straighten-only
    // carrier comes back through the corner conversion as the inscribed
    // rectangle's own corners, which land on 0/1 to within f32 rounding rather
    // than exactly on it. A 10⁻⁶ window is a tenth of a pixel on a 9504 px
    // frame — below anything a crop can mean.
    const FULL_EPS: f64 = 1e-6;
    let full = |c: &Crop| {
        c.left as f64 <= FULL_EPS
            && c.top as f64 <= FULL_EPS
            && c.right as f64 >= 1.0 - FULL_EPS
            && c.bottom as f64 >= 1.0 - FULL_EPS
    };
    // The rectangle read as the axis-aligned one it looks like. Correct at
    // `θ = 0`, where the corner encoding degenerates to exactly that and the
    // source frame IS the straightened frame — bit-stable, no aspect needed,
    // and the positive-extent guard is the `p > 0 ∧ q > 0` one at θ = 0.
    let verbatim = || {
        if !(lr.right > lr.left && lr.bottom > lr.top) {
            return CropDecode::Refused { straighten_deg };
        }
        let crop = Crop {
            left: lr.left as f32,
            top: lr.top as f32,
            right: lr.right as f32,
            bottom: lr.bottom as f32,
        };
        CropDecode::Read {
            crop: (!full(&crop)).then_some(crop),
            straighten_deg,
            overshoot_frac: 0.0,
        }
    };
    if lr.angle_deg == 0.0 {
        return verbatim();
    }
    let Some(s) = frame.map(|f| f.aspect()) else {
        // PROVENANCE RULE, the third (see `xmp_to_recipe`'s two): a tilted
        // rectangle in a document that declares no frame cannot be placed —
        // the corner decode needs `W/H` — but a document WE wrote without a
        // frame holds the rectangle in the straightened frame it was stored
        // in, because that is what this writer's own frameless arm emits.
        // Reading ours back verbatim is that arm's exact inverse; reading a
        // FOREIGN one that way would silently invent a rectangle out of
        // corners that mean something else, so it is dropped and disclosed.
        return if ours { verbatim() } else { CropDecode::NoFrame { straighten_deg } };
    };
    let (sin, cos) = lr.angle_deg.to_radians().sin_cos();
    // Signed half-extents of the stored corner pair, in units of the source
    // frame's HEIGHT (`W` and `H` cancel out of every fraction below, so the
    // aspect is the whole of what the frame contributes).
    let (xn, yn) = ((lr.right - lr.left) / 2.0 * s, (lr.bottom - lr.top) / 2.0);
    let (p, q) = (xn * cos + yn * sin, -xn * sin + yn * cos);
    if !(p > 0.0 && q > 0.0) {
        return CropDecode::Refused { straighten_deg };
    }
    let (wi, hi) = inscribed_norm(s, straighten_deg);
    if !(wi > 0.0 && hi > 0.0) {
        return CropDecode::Refused { straighten_deg };
    }
    // The crop centre, carried from the source frame into the straightened one
    // by the inverse of the rotation the renderer applies.
    let (ox, oy) = (((lr.left + lr.right) / 2.0 - 0.5) * s, (lr.top + lr.bottom) / 2.0 - 0.5);
    let (dx, dy) = (cos * ox + sin * oy, -sin * ox + cos * oy);
    let (x0, y0) = (wi / 2.0 + dx - p, hi / 2.0 + dy - q);
    let (left, right) = (x0 / wi, (x0 + 2.0 * p) / wi);
    let (top, bottom) = (y0 / hi, (y0 + 2.0 * q) / hi);
    // In units of the source height, like `wi`/`hi` themselves. A bare
    // maximum over the four fractions mixed two axes — `left`/`right` are
    // fractions of the inscribed WIDTH, `top`/`bottom` of its HEIGHT — and
    // forgot which one won, so no caller could turn it into pixels.
    let overshoot = [-left * wi, -top * hi, (right - 1.0) * wi, (bottom - 1.0) * hi]
        .into_iter()
        .fold(0.0f64, f64::max);
    let clamp01 = |v: f64| v.clamp(0.0, 1.0) as f32;
    let crop = Crop {
        left: clamp01(left),
        top: clamp01(top),
        right: clamp01(right),
        bottom: clamp01(bottom),
    };
    if !(crop.right > crop.left && crop.bottom > crop.top) {
        return CropDecode::Refused { straighten_deg };
    }
    CropDecode::Read {
        crop: (!full(&crop)).then_some(crop),
        straighten_deg,
        // In units of the source frame's height — the caller turns it into
        // pixels with the `tiff:ImageLength` it has.
        overshoot_frac: overshoot,
    }
}

/// This engine's `(Crop, straighten_deg)` → Lightroom's stored corners. The
/// exact inverse of [`lr_to_engine_crop`], and `R(θ)` is orthogonal, so the
/// round trip is algebraic rather than numerical.
///
/// `None` = this recipe has no crop and no tilt (`crs:HasCrop="False"`).
/// A straighten with no crop encodes as the WHOLE straightened frame — which
/// under the corner model is the inscribed rectangle's own four corners, not
/// `0,0,1,1` — because Adobe applies `CropAngle` only under `HasCrop="True"`.
/// At `straighten_deg == 0` the inscribed rectangle IS the frame and those
/// corners are `0,0,1,1` again, byte-identical to every document this writer
/// has ever produced.
///
/// The frameless arm is the one degraded edge: with a tilt and no aspect the
/// corners cannot be built, so the rectangle goes out in the STRAIGHTENED
/// frame it is stored in. Reachable only when the photo's own frame could not
/// be read (`pipeline::photo_frame_aspect` supplies it for every save that has
/// a photo), and it is what every build before R27 wrote for every crop.
pub(super) fn engine_to_lr_crop(
    crop: Option<&Crop>,
    straighten_deg: f64,
    frame: Option<FrameAspect>,
) -> Option<LrCrop> {
    if crop.is_none() && straighten_deg == 0.0 {
        return None;
    }
    let c = crop.copied().unwrap_or(Crop { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0 });
    let (l, t, r, b) = (c.left as f64, c.top as f64, c.right as f64, c.bottom as f64);
    let angle_deg = -straighten_deg;
    if straighten_deg == 0.0 {
        return Some(LrCrop { left: l, top: t, right: r, bottom: b, angle_deg });
    }
    let Some(s) = frame.map(|f| f.aspect()) else {
        return Some(LrCrop { left: l, top: t, right: r, bottom: b, angle_deg });
    };
    let (wi, hi) = inscribed_norm(s, straighten_deg);
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    let (p, q) = ((r - l) * wi / 2.0, (b - t) * hi / 2.0);
    let (dx, dy) = (((l + r) / 2.0 - 0.5) * wi, ((t + b) / 2.0 - 0.5) * hi);
    // Back into the source frame: the rotation itself, then the corners.
    let (ox, oy) = (cos * dx - sin * dy, sin * dx + cos * dy);
    let (cx, cy) = (ox + s / 2.0, oy + 0.5);
    let (x, y) = (p * cos - q * sin, p * sin + q * cos);
    Some(LrCrop {
        left: (cx - x) / s,
        right: (cx + x) / s,
        top: cy - y,
        bottom: cy + y,
        angle_deg,
    })
}

/// One document's crop block, decoded ([`lr_to_engine_crop`]). Adobe applies
/// `crs:CropAngle` only under `crs:HasCrop="True"`, so anything else is no crop
/// AND no tilt — importing a stale angle from a disabled crop activated a
/// straighten Adobe itself does not render.
pub(super) fn read_crop(scope: Scope<'_>, frame: Option<FrameAspect>, ours: bool) -> CropDecode {
    let angle = scope.crs_f32("CropAngle").unwrap_or(0.0) as f64;
    if scope.crs_str("HasCrop").as_deref() != Some("True") {
        return CropDecode::Read { crop: None, straighten_deg: 0.0, overshoot_frac: 0.0 };
    }
    let n = |k: &str| scope.crs_f32(k).map(f64::from);
    let (Some(left), Some(top), Some(right), Some(bottom)) =
        (n("CropLeft"), n("CropTop"), n("CropRight"), n("CropBottom"))
    else {
        // A crop block missing a corner is a rectangle we cannot place; the
        // tilt is a separate attribute and still rides. `unparsable_crs_numbers`
        // names the unreadable ones on its own channel.
        return CropDecode::Refused { straighten_deg: -angle };
    };
    lr_to_engine_crop(LrCrop { left, top, right, bottom, angle_deg: angle }, frame, ours)
}

/// What a document's crop cost on the way in, as one English sentence — the
/// crop half of the import disclosures `unparsable_crs_numbers` and
/// `import_losses` already carry for the global sliders and the masks.
///
/// `None` when the crop arrived whole, which is every uncropped document and
/// every un-straightened crop.
pub fn crop_import_note(xmp: &str) -> Option<String> {
    crop_import_note_in(xmp, None)
}

/// [`crop_import_note`] with the photograph beside the document, so an
/// orientation-only sidecar's rectangle is placed in the frame it was
/// written in ([`photo_frame_fallback`]) instead of being reported as
/// unplaceable — the same frame the recipe reader decodes it in, so the two
/// surfaces cannot disagree about one file.
pub fn crop_import_note_for_photo(xmp: &str, photo: &std::path::Path) -> Option<String> {
    crop_import_note_in(xmp, Some(photo))
}

fn crop_import_note_in(xmp: &str, photo: Option<&std::path::Path>) -> Option<String> {
    if xmp.len() > MAX_XMP_BYTES || xmlns_conflict(xmp).is_some() {
        return None;
    }
    let scope = crs_own_scope(xmp);
    let frame = FrameAspect::from_xmp(xmp).or_else(|| photo_frame_fallback(xmp, photo));
    match read_crop(Scope::new(scope.as_ref()), frame, is_autoshade_sidecar(xmp)) {
        CropDecode::Read { overshoot_frac, .. } if overshoot_frac > 0.0 => {
            // In pixels of the frame the crop was decoded in — a `Read` with
            // an overshoot implies one, since only a placed rectangle can
            // overshoot; a fraction means nothing to a photographer. The
            // decoder measures the overshoot in units of the source HEIGHT
            // (`CropDecode::Read`), so that is the side to scale by; the
            // longer side put every bottom-edge overshoot on a 3:2 frame 50 %
            // too high.
            let px = overshoot_frac * frame?.h;
            Some(format!(
                "the straightened crop reaches {px:.0} px outside the frame this build's \
                 straighten leaves behind, and was trimmed to fit"
            ))
        }
        CropDecode::NoFrame { .. } => Some(
            "the crop rectangle is tilted and the document declares no \
             tiff:ImageWidth/ImageLength, so its corners could not be placed — the straighten \
             was imported, the rectangle was not"
                .to_string(),
        ),
        CropDecode::Refused { .. } => Some(
            "the crop rectangle could not be read as a rectangle and was not imported"
                .to_string(),
        ),
        CropDecode::Read { .. } => None,
    }
}
