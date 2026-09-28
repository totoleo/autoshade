//! Distillation: the distilled fields, the preview, the style pull, and normalisation.

use super::*;

/// Every field [`blend_toward`] actually MOVED, as names a photographer can
/// look up on their own panel — bounded, and in a fixed order.
///
/// Measured from the two recipes rather than from the target map, which is the
/// honest direction: a target the proposal already sat on moved nothing, and a
/// note that listed it would be claiming a pull that did not happen. This is
/// the disclosure half of batch 2 — the note used to carry one percentage and
/// no answer at all to "toward what?".
///
/// Bounded at [`MAX_DISTILLED_FIELDS_CHARS`] with a `+N more` tail, for the
/// reason every other disclosure here is bounded: the rationale is persisted,
/// re-rendered in three UIs, and shares a 16 KiB ceiling with every other note.
/// A recipe with many masks can move a hundred fields.
pub fn distilled_fields(pre: &EditRecipe, post: &EditRecipe) -> String {
    use crate::advisor::catalogue;
    let mut names: Vec<String> = Vec::new();
    for (_, field) in style_targets_map() {
        let moved = match field {
            "exposure_ev" => pre.exposure_ev != post.exposure_ev,
            "contrast" => pre.contrast != post.contrast,
            "highlights" => pre.highlights != post.highlights,
            "shadows" => pre.shadows != post.shadows,
            "whites" => pre.whites != post.whites,
            "blacks" => pre.blacks != post.blacks,
            "vibrance" => pre.vibrance != post.vibrance,
            "clarity" => pre.clarity != post.clarity,
            "temperature_k" => pre.temperature_k != post.temperature_k,
            "tint" => pre.tint != post.tint,
            "saturation" => pre.saturation != post.saturation,
            "dehaze" => pre.dehaze != post.dehaze,
            _ => false,
        };
        if moved {
            names.push(field.to_string());
        }
    }
    for f in catalogue::hsl_expansion() {
        if catalogue::hsl_value(&pre.hsl, f.axis, f.band)
            != catalogue::hsl_value(&post.hsl, f.axis, f.band)
        {
            names.push(f.metric);
        }
    }
    for (field, _) in catalogue::COLOR_GRADE_CRS {
        if catalogue::color_grade_value(&pre.color_grade, field)
            != catalogue::color_grade_value(&post.color_grade, field)
        {
            names.push(format!("{COLOR_GRADE_LABEL}{field}"));
        }
    }
    if pre.tone_curve != post.tone_curve {
        names.push("tone_curve".to_string());
    }
    // Masks by POSITION, never by name: a mask's name is user text and this
    // string is persisted and shown (`autoshade-no-photo-filenames`).
    for (i, (a, b)) in pre.masks.iter().zip(&post.masks).enumerate() {
        for name in crate::mask_habit::HABIT_SLIDERS {
            let mut a = a.clone();
            let mut b = b.clone();
            let (Some(x), Some(y)) = (local_slider_mut(&mut a, name), local_slider_mut(&mut b, name))
            else {
                continue;
            };
            if x != y {
                names.push(format!("mask {} {name}", i + 1));
            }
        }
    }
    let mut out = String::new();
    for (i, n) in names.iter().enumerate() {
        let sep = if out.is_empty() { "" } else { ", " };
        if out.chars().count() + sep.len() + n.chars().count() > MAX_DISTILLED_FIELDS_CHARS {
            out.push_str(&format!("{sep}+{} more", names.len() - i));
            break;
        }
        out.push_str(sep);
        out.push_str(n);
    }
    out
}

/// The bound on [`distilled_fields`]. 384 characters is six 64-character rows
/// — about thirty field names, which covers every global channel plus a few
/// mask sliders before the `+N more` tail takes over — spent against the same
/// `rationale::MAX_RATIONALE` ceiling every other note is spent against.
pub const MAX_DISTILLED_FIELDS_CHARS: usize = 384;

/// What ONE retrieved neighbour set distils to, and what the Style dial would
/// do with it — printed, never rendered (A30).
///
/// The Style pull is the least inspectable thing the develop chain does: it
/// reads thirty-eight settings keys off four photographs, refuses most of them
/// through a consistency gate, and the only trace in the result is the
/// `STYLE_DISTILLED` sentence naming the fields that moved. A photographer
/// asking "why is my blue not being pulled?" had no way to see whether the
/// answer was "your neighbours disagree", "they never touched it" or "the dial
/// is at zero".
///
/// It re-derives NOTHING: the targets come from [`style_targets`] and the
/// applied line from [`blend_toward`] over a NEUTRAL recipe, which is the
/// production pair. So a preview that disagrees with a develop is a defect in
/// one of those two, not in a second implementation of them.
///
/// A NEUTRAL recipe rather than a real proposal, and the difference is stated
/// in the line itself: `blend_toward` interpolates from wherever the proposal
/// is toward the target, so the amount a real develop moves depends on what
/// the proposer said. From zero it is the whole pull, which is the upper bound
/// and the readable one.
pub fn distillation_preview(ex: &[&StyleExemplar], style: f32) -> String {
    use crate::advisor::catalogue;
    let targets = style_targets(ex);
    let pull = style_pull(style);
    let names: Vec<String> = neighbour_stems(ex);
    let mut out = format!(
        "distillation preview: {} neighbour(s) [{}], Style {style:.2} → pull {pull:.3}\n",
        ex.len(),
        names.join(", ")
    );
    let mono = ex.iter().filter(|e| e.mono).count();
    if mono > 0 {
        out.push_str(&format!(
            "  {mono} of them are black-and-white and take no part in the mixer\n"
        ));
    }
    if targets.is_empty() {
        out.push_str("  no target on any channel — this set agrees about nothing it was read for\n");
        return out;
    }
    let flat: Vec<String> =
        targets.sliders.iter().map(|(f, v)| format!("{f} {v:+.2}")).collect();
    out.push_str(&format!(
        "  sliders   {}\n",
        if flat.is_empty() { "none".to_string() } else { flat.join(", ") }
    ));
    for (axis, (axis_name, _)) in catalogue::HSL_AXES.iter().enumerate() {
        if axis == catalogue::HSL_AXIS_HUE {
            out.push_str("  mixer hue ingested, never distilled (see `style_targets`)\n");
            continue;
        }
        let cells: Vec<String> = catalogue::hsl_expansion()
            .into_iter()
            .filter(|f| f.axis == axis)
            .filter_map(|f| {
                let v = (*targets.hsl.get(axis)?.get(f.band)?)?;
                Some(format!("{} {v:+.2}", f.metric.rsplit('.').next().unwrap_or(&f.metric)))
            })
            .collect();
        out.push_str(&format!(
            "  mixer {axis_name:11} {} of 8 bands: {}\n",
            cells.len(),
            if cells.is_empty() { "no habit this set agrees on".to_string() } else { cells.join(", ") }
        ));
    }
    let wheels: Vec<String> = targets.grade.iter().map(|(f, v)| format!("{f} {v:+.2}")).collect();
    out.push_str(&format!(
        "  wheels    {}\n",
        if wheels.is_empty() { "no habit this set agrees on".to_string() } else { wheels.join(", ") }
    ));
    let curve: Vec<String> = ["black_lift", "s_strength"]
        .iter()
        .zip(targets.curve)
        .filter_map(|(n, v)| v.map(|v| format!("{n} {v:+.2}")))
        .collect();
    out.push_str(&format!(
        "  curve     {}\n",
        if curve.is_empty() { "no shape this set agrees on".to_string() } else { curve.join(", ") }
    ));
    for (slot, cells) in &targets.masks {
        let named: Vec<String> = crate::mask_habit::HABIT_SLIDERS
            .iter()
            .zip(cells)
            .filter_map(|(n, v)| v.map(|v| format!("{n} {v:+.2}")))
            .collect();
        let bucket = crate::mask_habit::Bucket::ALL
            .get(*slot)
            .map(|b| format!("{b:?}"))
            .unwrap_or_else(|| slot.to_string());
        out.push_str(&format!("  masks {bucket:7} {}\n", named.join(", ")));
    }
    // …and what the dial would actually MOVE, through the production blend.
    let neutral = EditRecipe::default();
    let mut pulled = neutral.clone();
    blend_toward(&mut pulled, &targets, pull);
    let moved = distilled_fields(&neutral, &pulled);
    out.push_str(&format!(
        "  applied to a NEUTRAL proposal at this Style: {}\n",
        if moved.is_empty() { "nothing moves".to_string() } else { moved }
    ));
    out
}

/// Style axis pull: preserve the shipped 0.3 default's historical 0.18 pull,
/// while allowing Style 1.0 to reach the retrieved target fully.
///
/// CONTINUOUS since 2026-09-24. The old `if s >= 0.5 { s } else { s * 0.6 }`
/// jumped from 0.294 to 0.500 between Style 0.49 and 0.50 — a slider tick
/// that moved the pull by 0.2. The three points anything documents or
/// renders against stay exactly where they were (0.3 → 0.18, 0.5 → 0.5,
/// 1.0 → 1.0), and nothing at or above 0.5 moves; the piece between 0.3
/// and 0.5 now rises straight from 0.18 to 0.5 instead of falling to 0.294.
pub fn style_pull(style: f32) -> f32 {
    let s = style.clamp(0.0, 1.0);
    if s < 0.3 {
        s * 0.6
    } else if s < 0.5 {
        0.18 + (s - 0.3) * 1.6
    } else {
        s
    }
}

pub(super) fn normalize(mut v: [f32; NDIM], mean: &[f32], std: &[f32]) -> [f32; NDIM] {
    for &d in &ZSCORE_DIMS {
        let s = std.get(d).copied().unwrap_or(1.0).max(1e-4);
        v[d] = (v[d] - mean.get(d).copied().unwrap_or(0.0)) / s;
    }
    v
}

pub(super) fn compute_norm(ex: &[StyleExemplar]) -> (Vec<f32>, Vec<f32>) {
    let mut mean = vec![0.0f32; NDIM];
    let mut std = vec![1.0f32; NDIM];
    if ex.is_empty() {
        return (mean, std);
    }
    let n = ex.len() as f32;
    for &d in &ZSCORE_DIMS {
        let m: f32 = ex.iter().map(|e| e.feat[d]).sum::<f32>() / n;
        let var: f32 = ex.iter().map(|e| (e.feat[d] - m).powi(2)).sum::<f32>() / n;
        mean[d] = m;
        std[d] = var.sqrt().max(1e-4);
    }
    (mean, std)
}
