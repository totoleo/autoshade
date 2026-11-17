//! The develop panel's custom widgets: the tone-curve editor, the parametric
//! curve, the colour mixer (HSL) grid with its Point Color rows, and the colour
//! grading wheels. They are real widget code, not form rows — a capability table
//! cannot draw a wheel — so they live in their own module instead of the panel.
//! The grading section's two whole-grade scalars (Blending/Balance) ARE form
//! rows and render through the capability table.

use crate::*;


/// Lightroom's parametric tone curve (v1.5.0), under the point curve in
/// the order the engine composes them (`render::build_tone_lut`: the point
/// curve bends what these regions made). The four regions in the panel's
/// top-down order, then the three splits — each bounded by its neighbours,
/// the way Lightroom's own split handles stop short of each other, so a
/// drag can never cross two splits and the sliders always show the layout
/// the curve renders (`EditRecipe::parametric_splits`).
pub(crate) fn parametric_curve(ui: &mut egui::Ui, lang: Lang, r: &mut EditRecipe) -> bool {
    use autoshade::recipe::{PARAMETRIC_SPLITS, PARAMETRIC_SPLIT_GAP};
    sub_caption(ui, tr(lang, K_DEVELOP_PARAMETRIC_CURVE));
    let mut changed = false;
    changed |= slider(ui, lang, tr(lang, K_DEVELOP_HIGHLIGHTS), &mut r.param_highlights, -100.0, 100.0, 0.0);
    changed |= slider(ui, lang, tr(lang, K_DEVELOP_LIGHTS), &mut r.param_lights, -100.0, 100.0, 0.0);
    changed |= slider(ui, lang, tr(lang, K_DEVELOP_DARKS), &mut r.param_darks, -100.0, 100.0, 0.0);
    changed |= slider(ui, lang, tr(lang, K_DEVELOP_SHADOWS), &mut r.param_shadows, -100.0, 100.0, 0.0);
    let gap = PARAMETRIC_SPLIT_GAP;
    let mut splits = r.parametric_splits();
    let mut moved = false;
    let labels = [tr(lang, K_DEVELOP_SHADOW_SPLIT), tr(lang, K_DEVELOP_MIDTONE_SPLIT), tr(lang, K_DEVELOP_HIGHLIGHT_SPLIT)];
    for (i, label) in labels.into_iter().enumerate() {
        // The reset lands inside the room the neighbours leave: a reset
        // must not cross them either (`slider_impl` resets unclamped).
        let (lo, hi, reset) =
            crate::logic::slider::split_bounds(i, splits, gap, PARAMETRIC_SPLITS);
        // Whole numbers whatever the room: the sidecar writes integers,
        // and a narrow range would otherwise pick the fractional feel.
        moved |= slider_impl(ui, lang, label, &mut splits[i], lo, hi, reset, SliderFeel::Int, "");
    }
    if moved {
        r.param_shadow_split = splits[0];
        r.param_midtone_split = splits[1];
        r.param_highlight_split = splits[2];
    }
    changed || moved
}
