//! Style targets: consistency, the exercised and habit means, the targets record, and blending toward them.

use super::*;

/// `(the settings label from [`REF_KEYS`]) → (the `EditRecipe` field name)`.
///
/// A named function since R23-1 so the registry-consistency test can read it:
/// this and `REF_KEYS` were two independent hand-kept lists, and nothing tied
/// either of them to the control they name.
pub(super) const fn style_targets_map() -> [(&'static str, &'static str); 12] {
    [
        ("exposure", "exposure_ev"),
        ("contrast", "contrast"),
        ("highlights", "highlights"),
        ("shadows", "shadows"),
        ("whites", "whites"),
        ("blacks", "blacks"),
        ("vibrance", "vibrance"),
        ("clarity", "clarity"),
        ("temperature_K", "temperature_k"),
        ("tint", "tint"),
        ("saturation", "saturation"),
        ("dehaze", "dehaze"),
    ]
}

/// How much of a retrieved population's magnitude must point the SAME WAY
/// before its mean counts as a HABIT rather than an artefact of cancellation.
///
/// The statistic is `rho = |mean| / mean(|v|)`. `rho == 1` is unanimity —
/// zeros included, because a neighbour who left an axis alone is not OPPOSING
/// it — and `rho == 0` is exact cancellation. In terms of opposing mass,
/// `rho >= k` means the minority side carries at most `(1 - k) / 2` of the
/// total; at 0.75 the majority outweighs it 7:1.
///
/// CALIBRATED, not picked (batch 2; the derivation and the per-band table are
/// in `r30-materials/quality/batch2-report.md`). Over the four real
/// four-neighbour sets the diagnosis measured, the DEFINED ratios of the
/// candidate keys are 0.037, 0.465, 0.587 | 0.942, 1.000: a gap between 0.587
/// and 0.942 whose midpoint is 0.765, and 0.75 is the round number inside it.
///
/// It sits high rather than in the middle of `[0, 1]` because the two errors
/// are not symmetric. Refusing a target leaves the proposal's own decision
/// standing and the rationale says which fields moved, so the cost is a style
/// pull that did less than it could. ACCEPTING a false one replaces a
/// scene-specific colour decision with a number that cancelled to near zero —
/// which is the failure this batch was opened for, one channel deeper.
pub const TARGET_CONSISTENCY: f32 = 0.75;

/// The two colour-grade fields that are MIXING parameters rather than colour
/// decisions.
///
/// `recipe::ColorGrade::is_neutral` states the rule this follows in its own
/// words — *blending/balance alone do nothing without a saturated or lifted
/// wheel* — so distilling them would move a control that changes nothing by
/// itself. `blending` also has no habit to learn: its library mean is ACR's own
/// default, present on 157 of 157 measured sidecars, i.e. a number nobody
/// chose.
const GRADE_NOT_A_DECISION: [&str; 2] = ["blending", "balance"];

/// The settings labels whose band has no negative side — a MAGNITUDE, where
/// "which way does the library lean" is a question with one possible answer.
///
/// Read off [`setting_bands`], never listed here: the bands are the recipe's
/// own clamps, so a control that gains or loses its negative side moves this
/// with it. Today it is the four colour-grade `*_sat` fields and `blending`
/// (0..100), and `temperature_K` (2000..40000), which reaches the plain-mean
/// path instead.
pub(super) fn label_is_one_sided(label: &str) -> bool {
    setting_bands().get(label).is_some_and(|&(lo, _)| lo >= 0.0)
}

/// The weighted mean of a population that EXERCISED the axis, or `None`.
///
/// [`consistent_mean`] for a one-sided quantity, minus the half of it that
/// cannot fail: with every value ≥ 0 the mean IS the mean of the absolute
/// values, so `rho` is exactly 1 whenever the axis was touched at all and the
/// direction gate can only ever pass. Applying it to a colour-grade `*_sat`
/// field therefore looked like a guard and was an identity — every non-zero
/// population produced a target, including `[0, 0, 0, 40]`, whose mean of 10
/// is one photographer's split tone spread over three who did not tone at all.
///
/// This asks the only question a magnitude can answer, and asks it out loud:
/// did the population touch the axis? The OUTPUT is unchanged for every
/// population a loaded index can hold — `setting_bands` clamps these labels to
/// 0..100 at the door, so `consistent_mean` returned this same number — and
/// what changes is that the code no longer states a test it cannot run. A
/// stricter rule (a minimum SHARE of the population that toned) is not
/// invented here because there is no calibration for one: on the user's index
/// the wheels' own participation is not a quantity this batch measured.
fn exercised_mean(vals: impl IntoIterator<Item = (f32, f32)>) -> Option<f32> {
    let (mut wsum, mut sum, mut abs) = (0.0f64, 0.0f64, 0.0f64);
    for (w, v) in vals {
        if !w.is_finite() || !v.is_finite() || w <= 0.0 {
            continue;
        }
        wsum += w as f64;
        sum += w as f64 * v as f64;
        abs += w as f64 * (v as f64).abs();
    }
    if wsum <= 0.0 || abs <= 0.0 {
        return None;
    }
    Some((sum / wsum) as f32)
}

/// The habit a population shows on ONE settings label, or `None` when it shows
/// none — the door [`style_targets`] reads for every label-keyed channel.
///
/// Which rule applies is decided by the label's own BAND ([`label_is_one_sided`])
/// rather than by where in the loop the question is asked, so a control that
/// gains a negative side starts being direction-gated without anyone
/// remembering to come back here.
fn habit_mean(label: &str, vals: impl IntoIterator<Item = (f32, f32)>) -> Option<f32> {
    if label_is_one_sided(label) { exercised_mean(vals) } else { consistent_mean(vals) }
}

/// The weighted mean of a population, or `None` when that mean is not a habit.
///
/// Two ways to answer `None`, and they are one rule — *do not claim a habit we
/// did not measure*:
///
/// * The population never exercised the axis (`mean(|v|) == 0`). `rho` is
///   `0 / 0` there: UNDEFINED, which is not the same as passing. Returning the
///   mean anyway would give every untouched band a target of `0`, and at Style
///   1.0 a target of `0` DELETES whatever the proposal decided. The twelve-slider
///   version of exactly that is why this batch exists; reproducing it
///   thirty-eight times over would have been the cure making the disease.
/// * The population contradicts itself (`|mean| < TARGET_CONSISTENCY * mean(|v|)`).
///   The mean of `+20` and `-18` is `+1`, and pulling a proposal onto `+1` is
///   not "distil my habit", it is erasing a decision and calling the wreckage a
///   style.
///
/// Weights are `amount`-style masses (1.0 for a plain per-exemplar value, the
/// bucket weight for a mask habit), so the mask half is the exact
/// `sum(w*mean)/sum(w)` that `mask_habit::BucketHabit::w` exists to make
/// possible rather than a mean of means. Non-finite and non-positive weights
/// are dropped at the door, like every other number that reaches a render.
fn consistent_mean(vals: impl IntoIterator<Item = (f32, f32)>) -> Option<f32> {
    let (mut wsum, mut sum, mut abs) = (0.0f64, 0.0f64, 0.0f64);
    for (w, v) in vals {
        if !w.is_finite() || !v.is_finite() || w <= 0.0 {
            continue;
        }
        wsum += w as f64;
        sum += w as f64 * v as f64;
        abs += w as f64 * (v as f64).abs();
    }
    if wsum <= 0.0 || abs <= 0.0 {
        return None;
    }
    let (mean, mabs) = (sum / wsum, abs / wsum);
    (mean.abs() >= TARGET_CONSISTENCY as f64 * mabs).then_some(mean as f32)
}

/// [`consistent_mean`] for an ANGLE: the weighted circular mean of a set of
/// degrees, or `None` when they do not agree on a direction.
///
/// A colour-grade wheel's hue is a point on a circle, and a wheel's hue is only
/// a CHOICE when that wheel carries saturation — an unsaturated wheel's `0` is
/// an untouched control, not a decision to tint red. Both facts are why this
/// cannot be `consistent_mean`, and the corpus shows the size of the error: on
/// one real neighbour set the shadow hues are `[0, 0, 229, 0]` with saturations
/// `[0, 0, 20, 0]`, so the arithmetic mean is 57° — a yellow-orange nobody
/// chose — where the saturation-weighted circular mean is the 229° blue the one
/// photograph that split-toned actually used.
///
/// The consistency statistic is the resultant length `R = |sum w*e^(i*theta)| /
/// sum w`, which is the circular analogue of `rho` and lands on the same scale:
/// `1` for identical angles, `0` for opposite ones. So the SAME
/// [`TARGET_CONSISTENCY`] applies, and does not need calibrating twice.
fn consistent_angle(vals: impl IntoIterator<Item = (f32, f32)>) -> Option<f32> {
    let (mut w, mut x, mut y) = (0.0f64, 0.0f64, 0.0f64);
    for (weight, deg) in vals {
        if !weight.is_finite() || !deg.is_finite() || weight <= 0.0 {
            continue;
        }
        let r = (deg as f64).to_radians();
        w += weight as f64;
        x += weight as f64 * r.cos();
        y += weight as f64 * r.sin();
    }
    if w <= 0.0 {
        return None;
    }
    if x.hypot(y) / w < TARGET_CONSISTENCY as f64 {
        return None;
    }
    let deg = y.atan2(x).to_degrees();
    Some(if deg < 0.0 { deg + 360.0 } else { deg } as f32)
}

/// Interpolate along the SHORTER arc, so a pull from 350° toward 10° passes
/// through 0° and not through 180°.
fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let d = ((b - a) % 360.0 + 540.0) % 360.0 - 180.0;
    let v = (a + d * t) % 360.0;
    if v < 0.0 { v + 360.0 } else { v }
}

/// Everything one retrieval says about how this photographer FINISHES a
/// photograph — the targets [`blend_toward`] pulls a proposal toward.
///
/// One mechanism, five channels. The mechanism has not changed since R23: a
/// target is the retrieved neighbours' mean, and the pull is `lerp(t)` with
/// `t = style_pull(style)`. What batch 2 changed is that the channels are no
/// longer just the flat twelve — see [`distil_keys`] for the ruling and the
/// asymmetry it fixes.
///
/// The channels, and what makes each one its own field rather than another row
/// in [`StyleTargets::sliders`]:
///
/// * [`sliders`] — the twelve flat globals. UNGATED, because they are the
///   shipped behaviour on an already-shipped release and nearly every sidecar
///   in a real library exercises them.
/// * [`hsl`] — the 8-band mixer by `(axis, band)`, gated. The `hue` axis is
///   INGESTED and never distilled; [`style_targets`] says why.
/// * [`grade`] — the wheels by `catalogue::COLOR_GRADE_CRS` field name, gated.
///   Angles live here too, and [`blend_toward`] tells them apart by name
///   because a wheel hue interpolates on a circle.
/// * [`curve`] — `[black_lift, s_strength]`, the shape
///   `eval::user_curve_shape` learns, gated per component. A SHAPE and never a
///   point list: the mean of four photographs' curves is a curve none of them
///   drew.
/// * [`masks`] — per `mask_habit::Bucket` (keyed by its index in
///   `Bucket::ALL`), one slot per `mask_habit::HABIT_SLIDERS` entry, gated.
///   AMPLITUDES ONLY. No coordinate is ever averaged, read or written; the
///   proposer places its own masks and this changes how hard they push.
///
/// `Option` per cell everywhere, and that is the load-bearing part: `None` is
/// "no habit measured, the proposal keeps its own decision", which is a
/// different fact from a measured `0.0`, and it is what an index built before
/// this vocabulary existed degrades to.
///
/// [`sliders`]: StyleTargets::sliders
/// [`hsl`]: StyleTargets::hsl
/// [`grade`]: StyleTargets::grade
/// [`curve`]: StyleTargets::curve
/// [`masks`]: StyleTargets::masks
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StyleTargets {
    /// The twelve flat globals, keyed by `EditRecipe` field name.
    pub sliders: BTreeMap<&'static str, f32>,
    /// `[axis][band]`, indexed as `catalogue::hsl_expansion` indexes them.
    pub hsl: [[Option<f32>; 8]; 3],
    /// Keyed by `catalogue::COLOR_GRADE_CRS` field name.
    pub grade: BTreeMap<&'static str, f32>,
    /// `[black_lift, s_strength]`, each gated on its own.
    pub curve: [Option<f32>; 2],
    /// Keyed by the bucket's index in `mask_habit::Bucket::ALL`; the vector is
    /// `mask_habit::HABIT_SLIDERS`-wide, READ FROM THE CONSTANT so a batch that
    /// grows that list does not have to come back here.
    pub masks: BTreeMap<usize, Vec<Option<f32>>>,
}

impl StyleTargets {
    /// Nothing to pull toward — the early return [`blend_toward`] takes.
    pub fn is_empty(&self) -> bool {
        self.sliders.is_empty()
            && self.grade.is_empty()
            && self.masks.is_empty()
            && self.curve.iter().all(Option::is_none)
            && self.hsl.iter().flatten().all(Option::is_none)
    }
}

/// What the retrieved exemplars agree this photographer does — the
/// "distil toward my historical style" target.
///
/// The twelve flat sliders are a plain mean, as they have been since R23. Every
/// OTHER channel goes through [`consistent_mean`] (or [`consistent_angle`]), so
/// a band the library contradicts itself on produces no target and the
/// proposal keeps its own decision there. Per key, independently: a
/// photographer can be unanimous about blues and undecided about greens, and
/// one summary number for "the mixer" would lose that.
///
/// **The `hue` axis of the 8-band mixer is ingested and never distilled.**
/// Mixer saturation and luminance change how strongly a colour READS; mixer
/// hue changes WHICH COLOUR IT IS, on whatever content happens to occupy that
/// band in THIS photograph.
///
/// MEASURED on the whole 169-exemplar library (v1.2.4), and the measurement
/// corrected the reason this switch used to give. It said the corpus showed
/// mixer hue to be SCENE-BOUND rather than habitual, on four four-neighbour
/// sets. Grouping the whole library by the vocabulary's own light class —
/// golden hour, blue hour, overcast, harsh midday, night, the only group in
/// [`LOOK_VOCAB`] that names the scene rather than the grade — the scene
/// explains 1.2 % to 7.8 % of each hue band's variance (η², 155 exemplars
/// carrying the mixer). It explains 1.1 % to 5.4 % of the two axes that ARE
/// distilled. Hue is no more scene-bound by lighting than saturation is, so
/// that reason does not hold and is not kept.
///
/// What DOES separate them is reach, and it is why the switch stays off.
/// Replaying the production ranking leave-one-out over all 169 exemplars, the
/// consistency gate would return a hue target on 8–49 % of the four-neighbour
/// sets, mean |target| 5.6–20.2 and a maximum of 55.75 on the red band. A
/// saturation target of that size makes a colour read more strongly; a hue
/// target of that size makes the reds orange, in whatever the frame happens to
/// put in the red band — and the fan veto v1.2.3 added exists because
/// per-band hue moves are the one control whose effect is a RELATIONSHIP
/// between the content and the grade rather than a property of the grade.
///
/// Recorded beside it, because it is the check that would license turning it
/// on and it fails: over the sets where the gate fires, the target's mean
/// absolute error against the held-out photograph's own value is 14.632 for
/// hue, 16.187 for saturation and 11.499 for luminance, against 6.943 /
/// 5.165 / 4.300 for leaving the control at 0. That is a statement about the
/// distillation as a whole and not about hue — [`blend_toward`] pulls a
/// PROPOSAL toward a habit on purpose and never claimed to predict the edit
/// this photographer would have made to this frame — so it is written down
/// rather than used to switch anything off.
///
/// The wheels are the opposite case and ARE distilled: a split-tone angle is
/// applied to a tonal RANGE with no content to re-identify, and the corpus
/// puts it at a stable 201–229° across neighbourhoods.
pub fn style_targets(ex: &[&StyleExemplar]) -> StyleTargets {
    use crate::advisor::catalogue;
    let mut out = StyleTargets::default();
    for (label, field) in style_targets_map() {
        let vals: Vec<f32> = ex.iter().filter_map(|e| e.settings.get(label).copied()).collect();
        if !vals.is_empty() {
            out.sliders.insert(field, vals.iter().sum::<f32>() / vals.len() as f32);
        }
    }
    let read = |label: &str| -> Vec<f32> {
        ex.iter().filter_map(|e| e.settings.get(label).copied()).collect()
    };
    // The eight-band mixer is read from the COLOUR exemplars only (A29): a
    // photograph the photographer converted to black and white has no colour
    // for that mixer to shape, so its cells describe a control its render did
    // not use. They are zeros, and `consistent_mean` counts a zero as
    // agreement rather than as opposition — so one monochrome neighbour in
    // four silently pulls every band target a quarter of the way to nothing
    // while never being able to refuse one. Excluded from the MIXER only: the
    // grade wheels are how a black-and-white frame is toned at all, and the
    // twelve flat sliders are the plain mean they have been since R23.
    let colour: Vec<&&StyleExemplar> = ex.iter().filter(|e| !e.mono).collect();
    let read_colour = |label: &str| -> Vec<f32> {
        colour.iter().filter_map(|e| e.settings.get(label).copied()).collect()
    };
    for f in catalogue::hsl_expansion() {
        if f.axis == catalogue::HSL_AXIS_HUE {
            continue;
        }
        if let Some(cell) = out.hsl.get_mut(f.axis).and_then(|a| a.get_mut(f.band)) {
            *cell = habit_mean(&f.metric, read_colour(&f.metric).into_iter().map(|v| (1.0, v)));
        }
    }
    // The wheels' INTENSITIES first: an angle is only learned for a wheel whose
    // intensity is itself a habit (the loop below reads this map back).
    for (field, _) in catalogue::COLOR_GRADE_CRS {
        if GRADE_NOT_A_DECISION.contains(&field) || catalogue::wheel_saturation_of(field).is_some()
        {
            continue;
        }
        let label = format!("{COLOR_GRADE_LABEL}{field}");
        if let Some(m) = habit_mean(&label, read(&label).into_iter().map(|v| (1.0, v))) {
            out.grade.insert(field, m);
        }
    }
    for (field, _) in catalogue::COLOR_GRADE_CRS {
        let Some(sat) = catalogue::wheel_saturation_of(field) else { continue };
        if !out.grade.contains_key(sat) {
            continue;
        }
        let (hue_label, sat_label) =
            (format!("{COLOR_GRADE_LABEL}{field}"), format!("{COLOR_GRADE_LABEL}{sat}"));
        let weighted = ex.iter().filter_map(|e| {
            Some((*e.settings.get(&sat_label)?, *e.settings.get(&hue_label)?))
        });
        if let Some(angle) = consistent_angle(weighted) {
            out.grade.insert(field, angle);
        }
    }
    let curves: Vec<[f32; 2]> = ex.iter().filter_map(|e| e.curve).collect();
    for (i, slot) in out.curve.iter_mut().enumerate() {
        *slot = consistent_mean(curves.iter().filter_map(|c| c.get(i).map(|v| (1.0, *v))));
    }
    for (slot, b) in crate::mask_habit::Bucket::ALL.iter().enumerate() {
        let per: Vec<Option<f32>> = (0..crate::mask_habit::N_HABIT_SLIDERS)
            .map(|i| {
                let weighted: Vec<(f32, f32)> = ex
                    .iter()
                    .filter_map(|e| e.masks.as_ref())
                    .map(|h| h.bucket(*b))
                    .filter(|h| h.w > 0.0)
                    .filter_map(|h| h.mean.get(i).map(|v| (h.w, *v)))
                    .collect();
                consistent_mean(weighted)
            })
            .collect();
        if per.iter().any(Option::is_some) {
            out.masks.insert(slot, per);
        }
    }
    out
}

/// One [`crate::mask_habit::HABIT_SLIDERS`] entry as a place on a
/// [`crate::recipe::LocalAdjustment`], or `None` when this build does not know
/// the name.
///
/// BY NAME, not by position, and that is the point. `HABIT_SLIDERS` is a
/// curated list that GROWS — it went from eight entries to ten in S3-B5, and
/// the advisor batch running beside this one adds `hue` — so a distillation
/// that indexed into it positionally would quietly start pulling the wrong
/// slider the day the list changed shape.
/// `every_habit_slider_is_addressable_on_a_local_adjustment` pins the other
/// half: that no name in the list is silently skipped here.
pub(super) fn local_slider_mut<'a>(
    m: &'a mut crate::recipe::LocalAdjustment,
    name: &str,
) -> Option<&'a mut f32> {
    Some(match name {
        "exposure" => &mut m.exposure_ev,
        "highlights" => &mut m.highlights,
        "shadows" => &mut m.shadows,
        "whites" => &mut m.whites,
        "blacks" => &mut m.blacks,
        "clarity" => &mut m.clarity,
        "dehaze" => &mut m.dehaze,
        "saturation" => &mut m.saturation,
        "temperature" => &mut m.temperature,
        "tint" => &mut m.tint,
        "hue" => &mut m.hue,
        _ => return None,
    })
}

/// Pull a proposal's master tone curve toward the library's curve HABIT.
///
/// The habit is a SHAPE — `[black_lift, s_strength]`, the pair
/// `eval::user_curve_shape` learns from a sidecar — and never a point list, for
/// the reason `eval` gives where it learns it: the mean of four photographs'
/// curves is a curve none of them drew. So the pull is applied to the shape and
/// written back THROUGH the points.
///
/// How the three promises are kept:
///
/// * **The domain.** Points at inputs 0, 64, 191 and 255 are ensured first, at
///   the values the curve already has there. Inserting a point ON a
///   piecewise-linear segment is an exact no-op, so this changes nothing yet —
///   it is what makes the black end movable and the two measurement points
///   FIXED.
/// * **Exactness.** `black_lift` is `lut[0]` and `s_strength` is
///   `(lut[191] - 191) - (lut[64] - 64)`. With 64 and 191 pinned as points,
///   moving output(0) cannot disturb either, and moving output(191) by `+d`
///   and output(64) by `-d` moves `s_strength` by exactly `2d`. So the shape
///   lands on the lerped target rather than near it.
/// * **Monotonicity, and the white end.** Every moved output is clamped
///   between its neighbours', so a pull can flatten a segment and can never
///   invert one; input 255 is measured by neither statistic and is never
///   written.
///
/// A pull smaller than the curve's own 1/255 resolution is dropped whole rather
/// than rewriting the point list to say the same thing — the same rule the
/// caller applies when it declines to disclose a distillation that changed
/// nothing.
fn blend_curve(recipe: &mut EditRecipe, target: [Option<f32>; 2], t: f32) {
    if target.iter().all(Option::is_none) {
        return;
    }
    let lut = crate::eval::recipe_curve_lut(recipe);
    let (black0, s0) = crate::eval::curve_shape(&lut);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let black = target[0].map_or(black0, |b| lerp(black0, b));
    let s = target[1].map_or(s0, |b| lerp(s0, b));
    if (black - black0).abs() < 0.5 && (s - s0).abs() < 0.5 {
        return;
    }
    // FIRST wins on a duplicated input — the rule `eval::curve_lut` and
    // `render::curve_lut` both follow, so the points this rebuilds from are the
    // points the engine would have rendered.
    let mut pts: BTreeMap<u8, f32> = BTreeMap::new();
    for p in &recipe.tone_curve {
        pts.entry(p.input).or_insert(p.output as f32);
    }
    for anchor in [0usize, 64, 191, 255] {
        pts.entry(anchor as u8).or_insert(lut[anchor]);
    }
    let half = (s - s0) / 2.0;
    if let Some(v) = pts.get_mut(&0) {
        *v = black;
    }
    if let Some(v) = pts.get_mut(&64) {
        *v -= half;
    }
    if let Some(v) = pts.get_mut(&191) {
        *v += half;
    }
    let mut floor = 0.0f32;
    let mut curve: Vec<crate::recipe::CurvePoint> = Vec::with_capacity(pts.len());
    for (input, output) in pts {
        let v = output.clamp(floor, 255.0);
        floor = v;
        curve.push(crate::recipe::CurvePoint { input, output: v.round() as u8 });
    }
    recipe.tone_curve = curve;
}

/// Pull `recipe` a fraction `t` (0..1) toward `targets` — this photographer's
/// measured habits. `t = 0` is a no-op.
///
/// **`t = 1` reaches the target.** `style_pull(1.0) == 1.0` and this is a
/// plain `lerp`, so at Style 100% a channel WITH a target ends on that target
/// and the proposal's own value for it is gone. That is the F1 ruling — the
/// dial goes all the way up — and it used to be contradicted by this very
/// doc comment, which promised a cap no caller applies. What keeps 100% from
/// meaning "delete the colour" is not a cap: it is that the vocabulary is now
/// symmetric (see [`distil_keys`]) and that a channel only HAS a target when
/// the library agreed on one (see [`consistent_mean`]). Everything that moves
/// is named in the rationale (`rationale::keys::STYLE_DISTILLED`).
pub fn blend_toward(recipe: &mut EditRecipe, targets: &StyleTargets, t: f32) {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.0 || targets.is_empty() {
        return;
    }
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    for (field, &target) in &targets.sliders {
        match *field {
            "exposure_ev" => recipe.exposure_ev = lerp(recipe.exposure_ev, target),
            "contrast" => recipe.contrast = lerp(recipe.contrast, target),
            "highlights" => recipe.highlights = lerp(recipe.highlights, target),
            "shadows" => recipe.shadows = lerp(recipe.shadows, target),
            "whites" => recipe.whites = lerp(recipe.whites, target),
            "blacks" => recipe.blacks = lerp(recipe.blacks, target),
            "vibrance" => recipe.vibrance = lerp(recipe.vibrance, target),
            "clarity" => recipe.clarity = lerp(recipe.clarity, target),
            "tint" => {
                // Tint pairs with Temperature (a tint is tuned AT a Kelvin,
                // and the index records the pair only under Custom WB) —
                // pulling tint onto an as-shot recipe applied HALF of a WB
                // decision as a floating colour cast. Same as-shot-stays
                // rule as temperature_k below.
                if recipe.temperature_k.is_some() {
                    recipe.tint = lerp(recipe.tint, target);
                }
            }
            "saturation" => recipe.saturation = lerp(recipe.saturation, target),
            "dehaze" => recipe.dehaze = lerp(recipe.dehaze, target),
            "temperature_k" => {
                // Blending needs a base Kelvin: an as-shot recipe (None) has
                // nothing to lerp FROM — `unwrap_or(target)` applied the
                // exemplar's temperature IN FULL at any nonzero strength and
                // silently flipped as-shot into custom WB. As-shot stays.
                if let Some(cur) = recipe.temperature_k {
                    recipe.temperature_k = Some(lerp(cur, target));
                }
            }
            _ => {}
        }
    }
    // The 8-band mixer, reached by (axis, band) through the registry rather
    // than by a private copy of the axis order.
    for (axis, bands) in targets.hsl.iter().enumerate() {
        for (band, target) in bands.iter().enumerate() {
            let (Some(target), Some(cur)) =
                (*target, crate::advisor::catalogue::hsl_value(&recipe.hsl, axis, band))
            else {
                continue;
            };
            if let Some(slot) = crate::advisor::catalogue::hsl_value_mut(&mut recipe.hsl, axis, band)
            {
                *slot = lerp(cur, target);
            }
        }
    }
    // The grade wheels. A `*_hue` field is an ANGLE and is told apart by the
    // registry, not by a name test written here.
    for (field, &target) in &targets.grade {
        let Some(cur) = crate::advisor::catalogue::color_grade_value(&recipe.color_grade, field)
        else {
            continue;
        };
        let next = match crate::advisor::catalogue::wheel_saturation_of(field) {
            None => lerp(cur, target),
            Some(sat) => {
                let intensity =
                    crate::advisor::catalogue::color_grade_value(&recipe.color_grade, sat)
                        .unwrap_or(0.0);
                if intensity == 0.0 {
                    // Nothing to lerp FROM: an unsaturated wheel has no tint,
                    // so its hue is an untouched control and not a decision
                    // this would be averaging with. The same rule the
                    // `temperature_k` arm above applies to an as-shot recipe,
                    // one control deeper — and it is also what keeps a
                    // half-strength pull from passing through a colour neither
                    // side asked for on its way to the target.
                    target
                } else {
                    lerp_angle(cur, target, t)
                }
            }
        };
        if let Some(slot) =
            crate::advisor::catalogue::color_grade_value_mut(&mut recipe.color_grade, field)
        {
            *slot = next;
        }
    }
    blend_curve(recipe, targets.curve, t);
    // The masks: AMPLITUDES ONLY. Nothing here reads or writes a coordinate,
    // a component, an amount or an enabled flag — `mask_habit`'s own rule, and
    // `distillation_never_moves_mask_geometry` is what holds it.
    if !targets.masks.is_empty() {
        for m in recipe.masks.iter_mut() {
            let bucket = crate::mask_habit::bucket_of(m);
            let Some(slot) = crate::mask_habit::Bucket::ALL.iter().position(|b| *b == bucket) else {
                continue;
            };
            let Some(per) = targets.masks.get(&slot) else { continue };
            for (i, name) in crate::mask_habit::HABIT_SLIDERS.iter().enumerate() {
                let Some(target) = per.get(i).copied().flatten() else { continue };
                let Some(cur) = local_slider_mut(m, name) else { continue };
                *cur = lerp(*cur, target);
            }
        }
    }
}
