//! Lightroom's brush, rasterised: the dab kernel, the `crs:Dabs` state machine, the raster and its cache.

use super::*;

// --- Lightroom's brush, rasterised ------------------------------------------
//
// Everything from here to `brush_raster` is ONE measurement made executable:
// R29 Batch-6 (`b6-analysis.md`, R29 materials ledger), 29 controlled
// Lightroom exports on one capture — a nine-rung hardness ladder (class 07,
// Δh = 0.125, one dab at the exact frame centre) and a 5 × 2 × 2 flow × radius
// × hardness grid of drags (class 06) — read back through batch-10's `par`
// composition law and un-warped into Lightroom's own pre-correction frame.
//
//     ρ       = |p − dab| / (Radius·W)          a dab is a circle in PIXELS
//     k(ρ;h)  = (1 − ρ^m(h))^n(h)               0 ≤ ρ < 1, else exactly 0
//     D(f)    = κf / (1 − f + κf)               κ = 0.1284 ± 0.0029, D(1) = 1
//     α       = 1 − Π_i (1 − value·D(f_i)·k(ρ_i; h_i))        SCREEN
//
// NOTHING here is a schema field. The raster is a render-time artefact keyed by
// the dab stream and the frame size, exactly as a `Bitmap` mask's decode is
// keyed by (path, mtime, size) — `recipe.json` gained no member and no
// `schema_era` gate moved.

/// Kernel exponents `(m, n)` of `k(ρ;h) = (1 − ρ^m)^n` at hardness `h`
/// (`crs:CenterWeight`, and the dab stream's `h` token — one quantity, two
/// spellings, agreeing to 4 dp on the census).
///
/// **Measured, not chosen** (B6 §4.4). Nine rungs, each fitted independently
/// with a profile-likelihood interval 5–15 % wide, so both parameters are
/// separately IDENTIFIED at every rung — which is what batch-10's rival
/// `disc ⊛ gaussian` was not (its disc radius jumped 0.002 → 0.503 between
/// adjacent rungs). `ln m` and `ln n` are then cubics in `h` fitted against all
/// nine at once: 8 numbers total, pooled rms 0.0102, **held-out 0.0109** (fit 5
/// rungs, predict the 4 interleaved ones) against 0.0180 for interpolating the
/// measured table itself. That is why this ships as a FORM and not as a table.
///
/// The physical reading (B6 §4.5): re-parametrised as `exp(−cρ^m)` the exponent
/// runs 1.95–2.15 at `h ≤ 0.25` — an actual gaussian — and 20.8 at `h = 1`, a
/// super-gaussian barely distinguishable from a top-hat. Hardness is the ORDER
/// of the falloff, not a plateau radius.
///
/// `h` is CLAMPED to Lightroom's own 0..1 before the cubics are evaluated. They
/// are empirical fits over exactly that interval and diverge outside it — at
/// `h = 2` they return `m = 5×10⁻⁴`, which would paint the whole frame. The
/// clamp is the guard, and it is the only one needed: on [0, 1] the cubics are
/// bounded at `m ∈ [1.66, 18.0]`, `n ∈ [1.40, 6.43]`.
///
/// Evaluated in f64 and returned as f32: the cubic coefficients carry six
/// decimals and `exp` of a sum near 2.9 loses the last of them in f32.
pub(super) fn brush_kernel_exponents(h: f32) -> (f32, f32) {
    // NaN clamps to NEITHER end — `f32::clamp` propagates it — and a NaN
    // exponent turns the whole raster into `0 as u8` further down, silently.
    // A hand-edited `recipe.json` is the only way to get one, and Lightroom's
    // own default `CenterWeight` is what it degrades to.
    let h = if h.is_nan() { 0.0 } else { f64::from(h).clamp(0.0, 1.0) };
    // Horner, from B6 §4.4:
    //   ln m(h) = −5.272996 h³ + 9.420690 h² − 1.864655 h + 0.605303
    //   ln n(h) = −5.976385 h³ + 12.872100 h² − 7.987943 h + 1.861162
    let ln_m = ((-5.272996 * h + 9.420690) * h - 1.864655) * h + 0.605303;
    let ln_n = ((-5.976385 * h + 12.872100) * h - 7.987943) * h + 1.861162;
    (ln_m.exp() as f32, ln_n.exp() as f32)
}

/// One dab's alpha profile: `k(ρ; h)`, the falloff at normalised radius `ρ`.
///
/// **Support ends EXACTLY at ρ = 1**, re-confirmed at nine hardnesses: α is
/// ≤ 5×10⁻⁴ in magnitude at every ρ ≥ 1.002 on every rung, so `crs:Radius` is
/// the outer support and not a half-width (B6 §4.1). Returning a hard 0 there
/// is therefore the measurement, not a convenience — and it is what keeps a
/// dab's cost proportional to its own area instead of the frame's.
///
/// `k(0) = 1` is likewise measured, not normalised in by hand: the
/// un-normalised peak reads 1.00288 ± 0.00229 across the nine rungs with no
/// trend in `h`, which is also an independent confirmation of `D(1) = 1`.
///
/// TESTS ONLY, and deliberately: the rasteriser hoists `brush_kernel_exponents`
/// out of its inner loop, so composing them per call is a convenience no pixel
/// path wants. The falloff ITSELF is [`brush_kernel_at`], which both this and
/// the rasteriser go through — so a mutation to the closed form still reaches
/// production, and the nine-rung pin still bites it.
#[cfg(test)]
pub(super) fn brush_kernel(rho: f32, h: f32) -> f32 {
    let (m, n) = brush_kernel_exponents(h);
    brush_kernel_at(rho * rho, m * 0.5, n)
}

/// [`brush_kernel`] with ρ² in hand and the exponents already resolved — the
/// form the rasteriser's inner loop wants, and the ONE place the falloff is
/// actually computed, so the pinned closed form and the pixels cannot drift.
///
/// `ρ^m = (ρ²)^(m/2)`: one exponentiation per texel, and no square root at all.
fn brush_kernel_at(rho2: f32, half_m: f32, n: f32) -> f32 {
    // NaN takes this branch too, deliberately: a NaN weight survives
    // `wgt <= 0.001` and casts to black (the same trap the radial arm's guarded
    // ramp documents), and an unreachable NaN is still a NaN once someone
    // hand-edits a `recipe.json`.
    if rho2.is_nan() || rho2 >= 1.0 {
        return 0.0;
    }
    if rho2 <= 0.0 {
        return 1.0;
    }
    (1.0 - rho2.powf(half_m)).max(0.0).powf(n)
}

/// The per-dab DEPOSIT at flow `f` — how much alpha one stamp lays down before
/// the screen accumulation folds the stamps together.
///
/// **A one-parameter ODDS law**, `D/(1−D) = κ·f/(1−f)` (B6 §5.3). Identified,
/// not fitted: over the sixteen unsaturated cells it scores rms 0.0030 against
/// 0.0207 for `1−(1−f)ⁿ` (6.9×) and 0.0383 for the naive linear `D = κf`
/// (12.7×), and giving the law a second free exponent returns n = 1.024 and
/// buys 0.0004. κ is UNIVERSAL to 2.24 % across a 3× radius change and both
/// hardness ends (0.12496 / 0.12804 / 0.12752 / 0.13293).
///
/// `D(1) = 1` is EXACT and free — `κ/(1−1+κ)` — which is also exact in f32
/// here, so a flow-1 dab deposits its full density with no epsilon. Pinning it
/// at 1 rather than fitting it costs at most 0.0037 on the four saturated
/// frames, below the α quantum (B6 §5.2).
///
/// **Registered wart, not swept** (B6 §5.4, §9): the per-rung κ rises ~11 %
/// from f = 0.10 to f = 0.75 in all four cells, so the law carries a small real
/// curvature no better one-parameter form absorbed. It is also why this κ
/// (four cells, 0.1284 ± 0.0029) sits 5.3 % above batch-10's single-cell
/// 0.12189 ± 0.00270 — about 1.6σ. **The two must not be quoted as agreeing to
/// better than 5 %.**
pub(super) fn brush_flow_deposit(f: f32) -> f32 {
    /// B6 §5.4, four cells: 0.12836 ± 0.00288. Supersedes batch-10's 0.12189.
    const KAPPA: f32 = 0.1284;
    let f = f.clamp(0.0, 1.0);
    let kf = KAPPA * f;
    let denom = 1.0 - f + kf;
    // Unreachable on [0, 1] — `denom ≥ κ > 0` — but a division that can only be
    // reasoned about is a division that eventually is not.
    if denom <= 0.0 { 1.0 } else { (kf / denom).clamp(0.0, 1.0) }
}

/// One stamp, fully resolved out of the token stream — position and radius in
/// the stored normalised frame, and the deposit and falloff in force when the
/// `d` token was reached.
///
/// The state tokens are resolved HERE and not in the pixel loop: `a` folds the
/// stroke density into `D(flow)` and `(half_m, n)` folds the hardness through
/// [`brush_kernel_exponents`], so the rasteriser's per-texel work is one
/// exponentiation and a multiply, and its per-(row, dab) work is a bounds test.
#[derive(Clone, Copy)]
struct BrushDab {
    /// `d <x>` — fraction of the frame WIDTH.
    x: f32,
    /// `d <y>` — fraction of the frame HEIGHT.
    y: f32,
    /// The current `r`, in WIDTH units on BOTH axes (a dab is a circle in
    /// pixels, so the y half-extent is `r·W/H` in normalised coordinates).
    r: f32,
    /// `BrushStroke::value · D(flow)` — the density-scaled deposit. Density
    /// scales the DAB, pre-screen (R27 Batch-8: the rival `min(MaskValue, ·)`
    /// cap reading is refuted at 13×).
    a: f32,
    /// `m(h)/2`, ready for the `ρ^m = (ρ²)^(m/2)` form.
    half_m: f32,
    /// `n(h)`.
    n: f32,
}

/// Total dabs one brush group may stamp. Four times the ENTIRE reference
/// library (15,964 dabs over 382 components) and ~100× its largest single
/// stroke (645). It is not reachable from a sidecar — `xmp::parse_dabs` caps a
/// stroke at 65,536 TOKENS and `recipe::clamp_strings` caps its stream at
/// 256 KiB — so only a hand-written `recipe.json` can meet it, and what it buys
/// is a hard ceiling on the rasteriser's work that does not depend on the
/// resolution search below.
const BRUSH_MAX_DABS: usize = 65_536;

/// Long edge of a brush alpha raster, before the work budget. Batch-10 §7.4's
/// own figure: at 2048 the 8-bit alpha quantum is 1/255 = 0.0039, which is
/// below the 0.0085 measurement quantum the whole model was fitted against, and
/// a dab's own transition width is ~5 raster px even at h = 1 (m = 18, so the
/// 10–90 % edge spans Δρ ≈ 0.05).
///
/// A raster is never UPSCALED past the frame it serves, so a 1280 px preview
/// gets a 1280 px raster and `sample_gray_norm`'s extent scaling then makes the
/// lookup an exact texel hit with no interpolation at all. Only a full-res
/// export downsamples (9504 → 2048, 4.6×), and what that costs is edge
/// sharpness on a dab far smaller than the ones this model was measured on.
const BRUSH_RASTER_MAX_EDGE: u32 = 2048;

/// Floor for the same edge. Below this the raster stops describing the mask at
/// all, so the work budget is allowed to be exceeded rather than the mask
/// destroyed — and `BRUSH_MAX_DABS` is what bounds the overrun.
pub(super) const BRUSH_RASTER_MIN_EDGE: u32 = 32;

/// Kernel evaluations one group may spend, which is what actually sets the
/// raster's resolution for a heavy stroke.
///
/// The work is `Σ_dabs min(π·(r·W_r)², W_r·H_r)` — each dab pays for its own
/// disc, clipped to the raster — and it scales as `s²` with the raster scale,
/// so the largest `s` meeting this budget is a closed form, not a search.
///
/// **The budget SELF-BALANCES, which is why one constant is enough.** A dab's
/// cost grows as `r²` while the resolution it NEEDS grows as `1/r`: coarsening
/// only ever blurs the dabs that were cheap. A 645-dab stroke at r = 0.05 (the
/// library's largest) costs 21 M evaluations and keeps the full 2048 raster; a
/// 645-dab stroke at r = 0.58 would ask for 1.8 G, and the 236 px raster the
/// budget hands it is ample for a mask whose every feature is 0.58 frame-widths
/// across.
///
/// **Sized against a measurement, not a guess.** The reference library's
/// largest real stroke — 645 dabs at r = 0.05 — rasterises for a 9504 × 6336
/// frame in **445 ms in a DEBUG build** on this machine (2048 × 1365 raster,
/// 2.13e7 evaluations, so ~21 ns each wall-clock across the row-parallel loop).
/// Straight-line proportionality puts a group that spends the whole budget at
/// ~0.5 s debug, once per (group, frame size) and then memoised.
///
/// **RELEASE, now measured — and the guess it replaces was wrong (R29 C3/C4).**
/// This line used to end "a release build is several times quicker again, and
/// it is not measured here". It is **~1.3×, not several times**: a synthetic
/// stroke of the same shape (645 dabs, r = 0.05, the same 0.2 r densification,
/// reproducing the documented 2.1e7 evaluations and the same 2048 × 1365
/// raster) rasterises in **416 ms release** (5 builds, 388–426 ms) against
/// **528 ms debug** (5 builds, 504–619 ms) in the SAME harness on the same
/// machine — each build a fresh dab stream, since `brush_raster` memoises on
/// (content, frame) and a repeat of one geometry would time the cache instead.
/// So the budget's headroom is the DEBUG figure's, and sizing this constant
/// against a hoped-for optimiser win would have been sizing it against nothing.
/// `-O` buys little here because the row loop is a bounds test and a multiply
/// over an 11 MB `prod` buffer spread across every core: it is bandwidth-bound,
/// not instruction-bound, which is also why the ratio is stated rather than
/// extrapolated to other machines.
///
/// The `BRUSH_MAX_DABS` × `BRUSH_RASTER_MIN_EDGE` corner — the only way past
/// this number, and reachable only from a hand-written `recipe.json` — bounds
/// out at 44 M evaluations.
pub(super) const BRUSH_RASTER_MAX_WORK: f64 = 24_000_000.0;

/// Process-wide cache budget for finished brush alphas, in bytes. Small on
/// purpose: at the 2048 edge one raster is 2.8 MB for a 3:2 frame (4.2 MB for a
/// square one), so this holds a handful and hard-resets rather than keeping LRU
/// books — the same trade `load_mask_bitmap`'s cache makes, and the same
/// reasoning, since a recipe holds a handful of masks.
///
/// **The memory accounting, stated rather than assumed.** Peak added by this
/// whole feature is `cache + one build transient` = 16 MB + (4·1 + 1)·4.2 Mpx
/// ≈ 37 MB, against the 256 MB `MASK_RASTER_BUDGET_BYTES` already reserves for
/// a SINGLE bitmap-mask decode. It rides inside that envelope with room to
/// spare and does not move `jobs::PER_PHOTO_PEAK_COMMIT_MB` (1800), which
/// budgets the develop's own f32 planes and never counted mask rasters.
const BRUSH_RASTER_CACHE_BYTES: usize = 16 * 1024 * 1024;

/// The brush raster's size for a frame of `fw × fh`, given `cost` = the
/// kernel evaluations stamping this group would take at FULL frame resolution
/// (`Σ min(π·(r·fw)², fw·fh)`).
///
/// Three bounds, in this order, and the third wins:
///
/// 1. [`BRUSH_RASTER_MAX_EDGE`], never upscaling past the frame itself;
/// 2. [`BRUSH_RASTER_MAX_WORK`] — work goes as `s²`, so the largest scale that
///    fits the budget is `sqrt(budget / cost)`, a closed form and not a search;
/// 3. [`BRUSH_RASTER_MIN_EDGE`], which OVERRIDES the budget: below it the
///    raster stops describing the mask at all, and `BRUSH_MAX_DABS` is what
///    bounds the overrun instead.
///
/// Split out of [`rasterise_brush_group`] so the policy can be asserted at a
/// cost the assertion does not have to pay — exercising the work budget by
/// actually rasterising costs, by construction, exactly the budget.
pub(super) fn brush_raster_dims(cost: f64, fw: u32, fh: u32) -> (u32, u32) {
    let long = f64::from(fw.max(fh)).max(1.0);
    let by_edge = (f64::from(BRUSH_RASTER_MAX_EDGE) / long).min(1.0);
    let by_work = if cost > 0.0 { (BRUSH_RASTER_MAX_WORK / cost).sqrt() } else { 1.0 };
    let floor = (f64::from(BRUSH_RASTER_MIN_EDGE) / long).min(1.0);
    let scale = by_edge.min(by_work).max(floor);
    (
        (f64::from(fw) * scale).round().max(1.0) as u32,
        (f64::from(fh) * scale).round().max(1.0) as u32,
    )
}

/// `<num>` off a dab token, or `None` — finite only.
pub(super) fn brush_token_num(it: &mut std::str::SplitWhitespace<'_>) -> Option<f32> {
    it.next().and_then(|t| t.parse::<f32>().ok()).filter(|v| v.is_finite())
}

/// The `crs:Dabs` state machine, run (`recipe::BrushStroke::dabs`). Four token
/// forms and no others — `r <f>` / `f <f>` / `h <f>` set the current state,
/// `d <x> <y>` stamps at it — with the stroke's own attributes as the INITIAL
/// state (measured: 102 components carry no `r` token at all and every one of
/// them has a non-zero `Radius` attribute).
///
/// **Nothing is interpolated.** Lightroom has already densified the polyline at
/// 0.2000·r (15,582 steps, IQR [0.1998, 0.2001], zero pen-lifts), so a renderer
/// stamps exactly the dabs it is given — which is also what makes the screen
/// accumulation's `N_eff` come out flat across the flow ladder (B6 §5.1).
///
/// Malformed tokens are SKIPPED, not refused. The XMP boundary already rejects
/// anything outside the grammar (`xmp::dab_token_is_known`, which is what makes
/// the round trip lossless); this parser also has to survive a hand-edited
/// `recipe.json`, where refusing the stroke would mean a silent whole-mask
/// change and skipping one token means a missing stamp.
fn brush_dabs(strokes: &[crate::recipe::BrushStroke], out: &mut Vec<BrushDab>) {
    for s in strokes {
        let value = s.value.clamp(0.0, 1.0);
        let (mut r, mut f, mut h) = (s.radius, s.flow, s.center_weight);
        for token in s.dabs.split('\n') {
            if out.len() >= BRUSH_MAX_DABS {
                return;
            }
            let mut it = token.split_whitespace();
            match it.next() {
                Some("r") => {
                    if let Some(v) = brush_token_num(&mut it) {
                        r = v;
                    }
                }
                Some("f") => {
                    if let Some(v) = brush_token_num(&mut it) {
                        f = v;
                    }
                }
                Some("h") => {
                    if let Some(v) = brush_token_num(&mut it) {
                        h = v;
                    }
                }
                Some("d") => {
                    let (Some(x), Some(y)) =
                        (brush_token_num(&mut it), brush_token_num(&mut it))
                    else {
                        continue;
                    };
                    let a = value * brush_flow_deposit(f);
                    // A zero radius or a zero deposit stamps nothing — and
                    // Lightroom draws nothing there either, so this is the
                    // model and not a shortcut. Dropping them here is what
                    // lets `rasterise_brush_group` answer `None` for a group
                    // that genuinely has no coverage.
                    if r.is_finite() && r > 0.0 && a > 0.0 {
                        let (m, n) = brush_kernel_exponents(h);
                        out.push(BrushDab { x, y, r, a, half_m: m * 0.5, n });
                    }
                }
                _ => {}
            }
        }
    }
}

/// Stamp one brush group's dab stream into an 8-bit grey alpha for a frame of
/// `fw × fh` pixels, or `None` when the group has no drawable dab.
///
/// **Pre-rasterised, not evaluated per pixel**, and the reason is arithmetic:
/// `mask_weight` runs per pixel and is called by up to five passes per mask, so
/// stamping N dabs inside it would be O(pixels × dabs × passes) — 5×10⁹ kernel
/// evaluations for a 90-dab stroke at 61 MP, before the passes multiply it.
/// Rasterising once costs `Σ` each dab's OWN disc and turns every later lookup
/// into the bilinear read a `Bitmap` mask already pays (batch-10 §7.4).
///
/// The accumulation carries the PRODUCT `Π(1 − a·k)` in f32 and converts once at
/// the end, so the screen law is applied in the form it was measured in and the
/// dab order cannot matter (it does not: the product is commutative, which is
/// itself part of why screen beat `max` by 3.4× and sum-clamp by 1.8×). That
/// commutativity is also what makes the row-parallel loop below sound without a
/// single lock — each row owns its slice, every dab is read-only, and no two
/// threads can disagree about a texel because none of them share one.
///
/// Coordinates are the STORED ones and are not warped — see the `Brush` arm of
/// `mask_weight` and `the_engine_evaluates_masks_before_the_geometry_stage`.
pub(super) fn rasterise_brush_group(
    strokes: &[crate::recipe::BrushStroke],
    fw: u32,
    fh: u32,
) -> Option<image::GrayImage> {
    if fw == 0 || fh == 0 {
        return None;
    }
    let mut dabs: Vec<BrushDab> = Vec::new();
    brush_dabs(strokes, &mut dabs);
    if dabs.is_empty() {
        return None;
    }
    // The cost of stamping this group at FULL frame resolution: each dab pays
    // for its own disc, clipped to the frame, which is what makes one absurd
    // radius cost a frame and not a universe. Saturating by construction.
    let frame_cells = f64::from(fw) * f64::from(fh);
    let cost: f64 = dabs
        .iter()
        .map(|d| {
            let disc = std::f64::consts::PI * (f64::from(d.r) * f64::from(fw)).powi(2);
            if disc.is_finite() { disc.min(frame_cells) } else { frame_cells }
        })
        .sum();
    let (rw, rh) = brush_raster_dims(cost, fw, fh);
    let (rwf, rhf) = (rw as f32, rh as f32);
    // A dab is a circle in PIXELS, so its y half-extent in normalised
    // coordinates is `r·W/H`. Read off the FRAME, not off the raster: rounding
    // rw and rh independently moves their ratio by up to a texel's worth.
    let aspect = fw as f32 / fh as f32;

    let (rwu, rhu) = (rw as usize, rh as usize);
    let mut prod = vec![1.0f32; rwu * rhu];
    // BY ROW, in parallel. Each row owns its slice and every dab is read-only,
    // so the accumulation needs no synchronisation at all — and because the
    // screen product is commutative, a row's result does not depend on which
    // thread reached it or in what order the dabs are folded in. The scan is
    // over ALL dabs per row (a bounds test each, ~10 flops), which is cheaper
    // than building and holding a per-row index for a list this size.
    prod.par_chunks_mut(rwu).enumerate().for_each(|(j, row)| {
        let jf = j as f32;
        for d in &dabs {
            // Centre and half-extents in TEXEL index space. Texel `i` owns the
            // normalised slice `[i/rw, (i+1)/rw]`, so its CENTRE is
            // `(i + MASK_SAMPLE_CENTRE)/rw` and a dab stored at the normalised
            // `d.x` sits at texel coordinate `d.x·rw − MASK_SAMPLE_CENTRE`.
            // That is the same grid `sample_gray_norm` reads back on, so a
            // same-size raster still costs no interpolation at all — and it is
            // the same grid the frame loop samples in, so the dab lands where
            // Lightroom puts it rather than half a pixel down and right
            // (R29 C2; both halves move, and their derivations are one).
            let (cx, cy) =
                (d.x * rwf - MASK_SAMPLE_CENTRE, d.y * rhf - MASK_SAMPLE_CENTRE);
            let (ex, ey) = (d.r * rwf, d.r * aspect * rhf);
            // A degenerate or overflowed extent has no disc to stamp. NaN is
            // caught by `is_finite`, so the comparisons only ever see a number.
            if !(ex.is_finite() && ey.is_finite()) || ex <= 0.0 || ey <= 0.0 {
                continue;
            }
            let dy = (jf - cy) / ey;
            let dy2 = dy * dy;
            if dy2 >= 1.0 {
                continue; // this row misses the dab entirely
            }
            // Clamped in f64 BEFORE the cast: an absurd hand-edited radius
            // makes these ±inf, and the range is clearer closed than saturated.
            let clamp_i = |v: f32| f64::from(v).clamp(0.0, f64::from(rw - 1)) as u32;
            let half = ex * (1.0 - dy2).sqrt(); // the chord, not the bbox
            let (i0, i1) = (clamp_i((cx - half).ceil()), clamp_i((cx + half).floor()));
            let sx = 1.0 / ex;
            for i in i0..=i1 {
                let dx = (i as f32 - cx) * sx;
                let rho2 = dx * dx + dy2;
                if rho2 >= 1.0 {
                    continue; // support ends EXACTLY at ρ = 1 (B6 §4.1)
                }
                let k = brush_kernel_at(rho2, d.half_m, d.n);
                row[i as usize] *= 1.0 - d.a * k;
            }
        }
    });
    let buf: Vec<u8> = prod
        .iter()
        .map(|p| ((1.0 - p).clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    image::GrayImage::from_raw(rw, rh, buf)
}

/// The brush alpha for one geometry at one frame size, through a process-wide
/// cache — the brush twin of [`load_mask_bitmap`], and cached for the same
/// reason: the GUI re-develops the preview on every slider tick, and stamping a
/// 645-dab stroke per tick would dominate the develop exactly as decoding the
/// segmentation PNG per tick used to.
///
/// `None` for a non-brush geometry (the caller asks about every geometry and
/// lets this one answer), and for a brush group with no drawable dab — which
/// renders inert, which is what Lightroom renders for the same group.
///
/// **The key is `(content hash, stroke count, stream bytes, fw, fh)`.** The
/// frame size belongs in it because a dab is a circle in pixels, so the same
/// stream is a different raster at a different aspect — and because the preview
/// and the export legitimately want different resolutions. The identity is a
/// 64-bit hash reinforced by two structural counts rather than a byte compare:
/// the alternative is holding a second copy of every dab stream (256 KiB per
/// stroke) for the life of the process. That is a weaker identity than a
/// content compare and it is stated as such; `load_mask_bitmap` accepts the
/// same shape of trade with (mtime, size).
pub(super) fn brush_raster(g: &MaskGeometry, fw: u32, fh: u32) -> Option<std::sync::Arc<image::GrayImage>> {
    use std::hash::{Hash, Hasher};
    use std::sync::{Arc, Mutex, OnceLock};
    let MaskGeometry::Brush { strokes, .. } = g else {
        return None;
    };
    if strokes.is_empty() {
        return None;
    }
    type Key = (u64, usize, usize, u32, u32);
    type Cache = Mutex<std::collections::HashMap<Key, Option<Arc<image::GrayImage>>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut bytes = 0usize;
    for s in strokes {
        // `to_bits`, not the float: f32 is not `Hash`, and the bit pattern is
        // the right identity here anyway — two streams that differ only in the
        // sign of a zero are two different streams to the sidecar writer.
        s.value.to_bits().hash(&mut hasher);
        s.radius.to_bits().hash(&mut hasher);
        s.flow.to_bits().hash(&mut hasher);
        s.center_weight.to_bits().hash(&mut hasher);
        s.dabs.hash(&mut hasher);
        bytes += s.dabs.len();
    }
    let key: Key = (hasher.finish(), strokes.len(), bytes, fw, fh);
    let cache = CACHE.get_or_init(Default::default);
    {
        // No user code runs under the lock, so poisoning is not reachable —
        // recover anyway rather than turning a past panic into a new one.
        let map = cache.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(hit) = map.get(&key) {
            return hit.clone();
        }
    }
    let built = rasterise_brush_group(strokes, fw, fh).map(Arc::new);
    {
        let mut map = cache.lock().unwrap_or_else(|p| p.into_inner());
        let held: usize =
            map.values().filter_map(|i| i.as_ref()).map(|i| i.as_raw().len()).sum();
        let incoming = built.as_ref().map_or(0, |i| i.as_raw().len());
        // A rare hard reset beats LRU bookkeeping for a handful of entries —
        // `load_mask_bitmap`'s cache makes the same call, in bytes as well as
        // entries, and for the same reason. The entry bound is what bounds the
        // `None` results (no drawable dabs): they weigh zero bytes, so without
        // it a stream of unique non-drawing groups would grow the map forever.
        if map.len() > 16 || held.saturating_add(incoming) > BRUSH_RASTER_CACHE_BYTES {
            map.clear();
        }
        map.insert(key, built.clone());
    }
    built
}
