//! The develop form's control registry: state in the recipe, behaviour behind
//! a trait.
//!
//! A control is a value implementing [`ControlWidget`]: it names its own
//! identity, label resource, band/reset, how it READS and WRITES the recipe,
//! and how it DRAWS itself. The renderer walks a table of trait objects and
//! calls `draw`, reporting a [`CapabilityChange`] (id + value) as data; the
//! view applies it by looking the id up in the same registry and calling
//! `apply`. Neither the renderer nor the command layer names a control.
//!
//! Extension: a new control of an existing shape is a row in its section's
//! table; a new KIND of control is a new struct + `impl ControlWidget` in its
//! own module plus its row. No enum, no central `match`, no command change.
//!
//! State stays where it belongs: the recipe (closed, versioned data) and the
//! double-click reset / band facts (`advisor::catalogue::form_control`). The
//! accessor pair is the row's binding to that state and lives INSIDE the
//! behaviour implementation, never in `Command`.

use autoshade::advisor::catalogue::form_control;
use autoshade::recipe::EditRecipe;
use autoshade::text::Key;
use crate::*;

/// How a slider feels to drag (the existing panel vocabulary).
pub(crate) enum ControlKind {
    Plain,
    Fine,
    CompanionInt,
    CompanionFine,
}

/// One edit a control reports; the view applies it through the registry.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CapabilityChange {
    pub control: &'static str,
    pub value: f32,
}

/// The stable abstraction the renderer and the apply path depend on.
pub(crate) trait ControlWidget: Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> Key;

    /// The band the slider draws plus the double-click reset, from the core
    /// catalogue (the table `EditRecipe::clamp` is pinned to).
    fn band_and_reset(&self) -> Option<((f32, f32), f32)> {
        let (band, reset) = form_control(self.id())?;
        Some((band, reset.unwrap_or(0.0)))
    }

    /// One disclosure line, where the control's number alone is not the story.
    fn hint(&self) -> Option<Key> {
        None
    }

    fn read(&self, recipe: &EditRecipe) -> f32;

    fn apply(&self, recipe: &mut EditRecipe, value: f32);

    /// Draw this control. Reports a change as data; writes happen in `apply`.
    fn draw(
        &self,
        ui: &mut Ui,
        lang: Lang,
        recipe: &EditRecipe,
        out: &mut Vec<CapabilityChange>,
    ) -> bool;
}

/// A plain recipe field: one slider, one read, one write.
pub(crate) struct Plain {
    id: &'static str,
    label: Key,
    kind: ControlKind,
    get: fn(&EditRecipe) -> f32,
    set: fn(&mut EditRecipe, f32),
}

impl Plain {
    pub(crate) const fn new(
        id: &'static str,
        label: Key,
        kind: ControlKind,
        get: fn(&EditRecipe) -> f32,
        set: fn(&mut EditRecipe, f32),
    ) -> Self {
        Self { id, label, kind, get, set }
    }
}

impl ControlWidget for Plain {
    fn id(&self) -> &'static str {
        self.id
    }

    fn label(&self) -> Key {
        self.label
    }

    fn read(&self, recipe: &EditRecipe) -> f32 {
        (self.get)(recipe)
    }

    fn apply(&self, recipe: &mut EditRecipe, value: f32) {
        (self.set)(recipe, value)
    }

    fn draw(
        &self,
        ui: &mut Ui,
        lang: Lang,
        recipe: &EditRecipe,
        out: &mut Vec<CapabilityChange>,
    ) -> bool {
        let Some(((min, max), reset)) = self.band_and_reset() else { return false };
        let mut current = self.read(recipe);
        let text = tr(lang, self.label());
        let dirty = match self.kind {
            ControlKind::Fine => slider_fine(ui, lang, text, &mut current, min, max, reset),
            _ => slider(ui, lang, text, &mut current, min, max, reset),
        };
        if dirty {
            out.push(CapabilityChange { control: self.id, value: current });
        }
        dirty
    }
}

/// A plain field whose slider carries one disclosure line.
pub(crate) struct Hinted {
    id: &'static str,
    label: Key,
    hint: Key,
    get: fn(&EditRecipe) -> f32,
    set: fn(&mut EditRecipe, f32),
}

impl Hinted {
    pub(crate) const fn new(
        id: &'static str,
        label: Key,
        hint: Key,
        get: fn(&EditRecipe) -> f32,
        set: fn(&mut EditRecipe, f32),
    ) -> Self {
        Self { id, label, hint, get, set }
    }
}

impl ControlWidget for Hinted {
    fn id(&self) -> &'static str {
        self.id
    }

    fn label(&self) -> Key {
        self.label
    }

    fn hint(&self) -> Option<Key> {
        Some(self.hint)
    }

    fn read(&self, recipe: &EditRecipe) -> f32 {
        (self.get)(recipe)
    }

    fn apply(&self, recipe: &mut EditRecipe, value: f32) {
        (self.set)(recipe, value)
    }

    fn draw(
        &self,
        ui: &mut Ui,
        lang: Lang,
        recipe: &EditRecipe,
        out: &mut Vec<CapabilityChange>,
    ) -> bool {
        let Some(((min, max), reset)) = self.band_and_reset() else { return false };
        let mut current = self.read(recipe);
        let hint = self.hint().map_or("", |hint| tr(lang, hint));
        let dirty = slider_hinted(
            ui,
            lang,
            tr(lang, self.label()),
            &mut current,
            min,
            max,
            reset,
            hint,
        );
        if dirty {
            out.push(CapabilityChange { control: self.id, value: current });
        }
        dirty
    }
}

/// A Lightroom COMPANION: a stored 0 means "the sidecar said nothing", read
/// and written through `EditRecipe::resolved` / `set_resolved`.
pub(crate) struct Companion {
    id: &'static str,
    label: Key,
    kind: ControlKind,
}

impl Companion {
    pub(crate) const fn new(id: &'static str, label: Key, kind: ControlKind) -> Self {
        Self { id, label, kind }
    }
}

impl ControlWidget for Companion {
    fn id(&self) -> &'static str {
        self.id
    }

    fn label(&self) -> Key {
        self.label
    }

    fn read(&self, recipe: &EditRecipe) -> f32 {
        recipe.resolved(self.id)
    }

    fn apply(&self, recipe: &mut EditRecipe, value: f32) {
        let default = autoshade::recipe::LR_COMPANION_DEFAULTS
            .iter()
            .find(|(name, _)| *name == self.id)
            .map_or(0.0, |(_, default)| *default);
        if value == default {
            recipe.clear_resolved(self.id);
        } else {
            recipe.set_resolved(self.id, value);
        }
    }

    fn draw(
        &self,
        ui: &mut Ui,
        lang: Lang,
        recipe: &EditRecipe,
        out: &mut Vec<CapabilityChange>,
    ) -> bool {
        let Some(((min, max), _reset)) = self.band_and_reset() else { return false };
        let feel = if matches!(self.kind, ControlKind::CompanionFine) {
            SliderFeel::Fine
        } else {
            SliderFeel::Int
        };
        match companion_slider_edit(ui, lang, tr(lang, self.label), recipe, self.id, min, max, feel) {
            Some(value) => {
                out.push(CapabilityChange { control: self.id, value });
                true
            }
            None => false,
        }
    }
}

const fn plain(
    id: &'static str,
    label: Key,
    kind: ControlKind,
    get: fn(&EditRecipe) -> f32,
    set: fn(&mut EditRecipe, f32),
) -> Plain {
    Plain::new(id, label, kind, get, set)
}

const fn hinted(
    id: &'static str,
    label: Key,
    hint: Key,
    get: fn(&EditRecipe) -> f32,
    set: fn(&mut EditRecipe, f32),
) -> Hinted {
    Hinted::new(id, label, hint, get, set)
}

const fn companion(id: &'static str, label: Key, kind: ControlKind) -> Companion {
    Companion::new(id, label, kind)
}
/// TONE.
pub(crate) static TONE: &[&dyn ControlWidget] = &[
    &plain("exposure_ev", K_DEVELOP_EXPOSURE, ControlKind::Plain, |r| r.exposure_ev, |r, v| r.exposure_ev = v),
    &plain("contrast", K_DEVELOP_CONTRAST, ControlKind::Plain, |r| r.contrast, |r, v| r.contrast = v),
    &plain("highlights", K_DEVELOP_HIGHLIGHTS, ControlKind::Plain, |r| r.highlights, |r, v| r.highlights = v),
    &plain("shadows", K_DEVELOP_SHADOWS, ControlKind::Plain, |r| r.shadows, |r, v| r.shadows = v),
    &plain("whites", K_DEVELOP_WHITES, ControlKind::Plain, |r| r.whites, |r, v| r.whites = v),
    &plain("blacks", K_DEVELOP_BLACKS, ControlKind::Plain, |r| r.blacks, |r, v| r.blacks = v),
];

/// PRESENCE.
pub(crate) static PRESENCE: &[&dyn ControlWidget] = &[
    &plain("texture", K_DEVELOP_TEXTURE, ControlKind::Plain, |r| r.texture, |r, v| r.texture = v),
    &plain("clarity", K_DEVELOP_CLARITY, ControlKind::Plain, |r| r.clarity, |r, v| r.clarity = v),
    &plain("dehaze", K_DEVELOP_DEHAZE, ControlKind::Plain, |r| r.dehaze, |r, v| r.dehaze = v),
    &plain("vibrance", K_DEVELOP_VIBRANCE, ControlKind::Plain, |r| r.vibrance, |r, v| r.vibrance = v),
    &plain("saturation", K_DEVELOP_SATURATION, ControlKind::Plain, |r| r.saturation, |r, v| r.saturation = v),
];

/// VIGNETTE.
pub(crate) static VIGNETTE: &[&dyn ControlWidget] = &[
    &plain("post_crop_vignette", K_DEVELOP_VIGNETTE_AMOUNT, ControlKind::Plain, |r| r.post_crop_vignette, |r, v| r.post_crop_vignette = v),
    &companion("post_crop_vignette_mid", K_DEVELOP_MIDPOINT, ControlKind::CompanionInt),
    &companion("post_crop_vignette_feather", K_DEVELOP_VIGNETTE_FEATHER, ControlKind::CompanionInt),
    &plain("post_crop_vignette_round", K_DEVELOP_VIGNETTE_ROUNDNESS, ControlKind::Plain, |r| r.post_crop_vignette_round, |r, v| r.post_crop_vignette_round = v),
    &companion("post_crop_vignette_style", K_DEVELOP_VIGNETTE_STYLE, ControlKind::CompanionInt),
    &plain("post_crop_vignette_hl", K_DEVELOP_VIGNETTE_HIGHLIGHTS, ControlKind::Plain, |r| r.post_crop_vignette_hl, |r, v| r.post_crop_vignette_hl = v),
];

/// GRAIN.
pub(crate) static GRAIN: &[&dyn ControlWidget] = &[
    &plain("grain", K_DEVELOP_GRAIN_AMOUNT, ControlKind::Plain, |r| r.grain, |r, v| r.grain = v),
    &companion("grain_size", K_DEVELOP_GRAIN_SIZE, ControlKind::CompanionInt),
    &companion("grain_rough", K_DEVELOP_GRAIN_ROUGHNESS, ControlKind::CompanionInt),
];

/// HDR_HEADROOM.
pub(crate) static HDR_HEADROOM: &[&dyn ControlWidget] = &[
    &plain("hdr_max_ev", K_DEVELOP_HEADROOM_STOPS, ControlKind::Fine, |r| r.hdr_max_ev, |r, v| r.hdr_max_ev = v),
];

/// HDR_SDR.
pub(crate) static HDR_SDR: &[&dyn ControlWidget] = &[
    &plain("sdr_blend", K_DEVELOP_BLEND, ControlKind::Plain, |r| r.sdr_blend, |r, v| r.sdr_blend = v),
    &plain("sdr_brightness", K_DEVELOP_BRIGHTNESS, ControlKind::Plain, |r| r.sdr_brightness, |r, v| r.sdr_brightness = v),
    &plain("sdr_contrast", K_DEVELOP_CONTRAST, ControlKind::Plain, |r| r.sdr_contrast, |r, v| r.sdr_contrast = v),
    &plain("sdr_highlights", K_DEVELOP_HIGHLIGHTS, ControlKind::Plain, |r| r.sdr_highlights, |r, v| r.sdr_highlights = v),
    &plain("sdr_shadows", K_DEVELOP_SHADOWS, ControlKind::Plain, |r| r.sdr_shadows, |r, v| r.sdr_shadows = v),
    &plain("sdr_whites", K_DEVELOP_WHITES, ControlKind::Plain, |r| r.sdr_whites, |r, v| r.sdr_whites = v),
    &plain("sdr_clarity", K_DEVELOP_CLARITY, ControlKind::Plain, |r| r.sdr_clarity, |r, v| r.sdr_clarity = v),
];

/// LENS_MANUAL.
pub(crate) static LENS_MANUAL: &[&dyn ControlWidget] = &[
    &plain("lens_vignette", K_DEVELOP_LENS_VIGNETTING, ControlKind::Plain, |r| r.lens_vignette, |r, v| r.lens_vignette = v),
    &plain("lens_vignette_mid", K_DEVELOP_MIDPOINT, ControlKind::Plain, |r| r.lens_vignette_mid, |r, v| r.lens_vignette_mid = v),
    &plain("lens_distortion", K_DEVELOP_DISTORTION, ControlKind::Plain, |r| r.lens_distortion, |r, v| r.lens_distortion = v),
];

/// LENS_PROFILE.
pub(crate) static LENS_PROFILE: &[&dyn ControlWidget] = &[
    &hinted("lens_profile_distortion_scale", K_DEVELOP_DISTORTION_AMOUNT, K_DEVELOP_100_EXACTLY_WHAT_THE_PROFILE_SAYS_0_SWITCHES_THA, |r| r.lens_profile_distortion_scale, |r, v| r.lens_profile_distortion_scale = v),
    &hinted("lens_profile_vignetting_scale", K_DEVELOP_VIGNETTING_AMOUNT, K_DEVELOP_100_EXACTLY_WHAT_THE_PROFILE_SAYS_0_SWITCHES_THA, |r| r.lens_profile_vignetting_scale, |r, v| r.lens_profile_vignetting_scale = v),
];

/// LENS_CA.
pub(crate) static LENS_CA: &[&dyn ControlWidget] = &[
    &plain("ca_r", K_DEVELOP_RED_CYAN, ControlKind::Plain, |r| r.ca_r, |r, v| r.ca_r = v),
    &plain("ca_b", K_DEVELOP_BLUE_YELLOW, ControlKind::Plain, |r| r.ca_b, |r, v| r.ca_b = v),
];

/// LENS_DEFRINGE.
pub(crate) static LENS_DEFRINGE: &[&dyn ControlWidget] = &[
    &plain("defringe_purple", K_DEVELOP_PURPLE_AMOUNT, ControlKind::Plain, |r| r.defringe_purple, |r, v| r.defringe_purple = v),
    &hinted("defringe_purple_lo", K_DEVELOP_PURPLE_HUE_LOW, K_DEVELOP_A_HUE_WINDOW_CORRECTS_NOTHING_UNTIL_ITS_AMOUNT_I, |r| r.defringe_purple_lo, |r, v| r.defringe_purple_lo = v),
    &hinted("defringe_purple_hi", K_DEVELOP_PURPLE_HUE_HIGH, K_DEVELOP_A_HUE_WINDOW_CORRECTS_NOTHING_UNTIL_ITS_AMOUNT_I, |r| r.defringe_purple_hi, |r, v| r.defringe_purple_hi = v),
    &plain("defringe_green", K_DEVELOP_GREEN_AMOUNT, ControlKind::Plain, |r| r.defringe_green, |r, v| r.defringe_green = v),
    &hinted("defringe_green_lo", K_DEVELOP_GREEN_HUE_LOW, K_DEVELOP_A_HUE_WINDOW_CORRECTS_NOTHING_UNTIL_ITS_AMOUNT_I, |r| r.defringe_green_lo, |r, v| r.defringe_green_lo = v),
    &hinted("defringe_green_hi", K_DEVELOP_GREEN_HUE_HIGH, K_DEVELOP_A_HUE_WINDOW_CORRECTS_NOTHING_UNTIL_ITS_AMOUNT_I, |r| r.defringe_green_hi, |r, v| r.defringe_green_hi = v),
];

/// CAL_TINT.
pub(crate) static CAL_TINT: &[&dyn ControlWidget] = &[
    &plain("cal_shadow_tint", K_DEVELOP_TINT, ControlKind::Plain, |r| r.cal_shadow_tint, |r, v| r.cal_shadow_tint = v),
];

/// CAL_RED.
pub(crate) static CAL_RED: &[&dyn ControlWidget] = &[
    &plain("cal_red_hue", K_DEVELOP_HUE, ControlKind::Plain, |r| r.cal_red_hue, |r, v| r.cal_red_hue = v),
    &plain("cal_red_sat", K_DEVELOP_SATURATION, ControlKind::Plain, |r| r.cal_red_sat, |r, v| r.cal_red_sat = v),
];

/// CAL_GREEN.
pub(crate) static CAL_GREEN: &[&dyn ControlWidget] = &[
    &plain("cal_green_hue", K_DEVELOP_HUE, ControlKind::Plain, |r| r.cal_green_hue, |r, v| r.cal_green_hue = v),
    &plain("cal_green_sat", K_DEVELOP_SATURATION, ControlKind::Plain, |r| r.cal_green_sat, |r, v| r.cal_green_sat = v),
];

/// CAL_BLUE.
pub(crate) static CAL_BLUE: &[&dyn ControlWidget] = &[
    &plain("cal_blue_hue", K_DEVELOP_HUE, ControlKind::Plain, |r| r.cal_blue_hue, |r, v| r.cal_blue_hue = v),
    &plain("cal_blue_sat", K_DEVELOP_SATURATION, ControlKind::Plain, |r| r.cal_blue_sat, |r, v| r.cal_blue_sat = v),
];

/// WB_TINT.
pub(crate) static WB_TINT: &[&dyn ControlWidget] = &[
    &plain("tint", K_DEVELOP_TINT, ControlKind::Plain, |r| r.tint, |r, v| r.tint = v),
];

/// GRADE_GLOBALS.
pub(crate) static GRADE_GLOBALS: &[&dyn ControlWidget] = &[
    &plain("color_grade.blending", K_DEVELOP_BLENDING, ControlKind::Plain, |r| r.color_grade.blending, |r, v| r.color_grade.blending = v),
    &plain("color_grade.balance", K_DEVELOP_BALANCE, ControlKind::Plain, |r| r.color_grade.balance, |r, v| r.color_grade.balance = v),
];

/// CROP.
pub(crate) static CROP: &[&dyn ControlWidget] = &[
    &plain("straighten_deg", K_DEVELOP_STRAIGHTEN, ControlKind::Fine, |r| r.straighten_deg, |r, v| r.straighten_deg = v),
];

/// SHARPEN.
pub(crate) static SHARPEN: &[&dyn ControlWidget] = &[
    &companion("sharpen_radius", K_DEVELOP_SHARPEN_RADIUS, ControlKind::CompanionFine),
    &companion("sharpen_detail", K_DEVELOP_SHARPEN_DETAIL, ControlKind::CompanionInt),
    &plain("sharpen_mask", K_DEVELOP_SHARPEN_MASKING, ControlKind::Plain, |r| r.sharpen_mask, |r, v| r.sharpen_mask = v),
];

/// NOISE_LUMA.
pub(crate) static NOISE_LUMA: &[&dyn ControlWidget] = &[
    &plain("noise_reduction", K_DEVELOP_NOISE_REDUCTION, ControlKind::Plain, |r| r.noise_reduction, |r, v| r.noise_reduction = v),
    &companion("nr_detail", K_DEVELOP_NOISE_DETAIL, ControlKind::CompanionInt),
    &plain("nr_contrast", K_DEVELOP_NOISE_CONTRAST, ControlKind::Plain, |r| r.nr_contrast, |r, v| r.nr_contrast = v),
];

/// NOISE_COLOR.
pub(crate) static NOISE_COLOR: &[&dyn ControlWidget] = &[
    &plain("color_nr", K_DEVELOP_COLOUR_NOISE_REDUCTION, ControlKind::Plain, |r| r.color_nr, |r, v| r.color_nr = v),
    &companion("color_nr_detail", K_DEVELOP_COLOUR_NOISE_DETAIL, ControlKind::CompanionInt),
    &companion("color_nr_smooth", K_DEVELOP_COLOUR_NOISE_SMOOTHNESS, ControlKind::CompanionInt),
];

/// TRANSFORM.
pub(crate) static TRANSFORM: &[&dyn ControlWidget] = &[
    &plain("perspective_vertical", K_DEVELOP_VERTICAL, ControlKind::Plain, |r| r.perspective_vertical, |r, v| r.perspective_vertical = v),
    &plain("perspective_horizontal", K_DEVELOP_HORIZONTAL, ControlKind::Plain, |r| r.perspective_horizontal, |r, v| r.perspective_horizontal = v),
    &plain("perspective_rotate", K_DEVELOP_ROTATE, ControlKind::Plain, |r| r.perspective_rotate, |r, v| r.perspective_rotate = v),
    &plain("perspective_scale", K_DEVELOP_TRANSFORM_SCALE, ControlKind::Plain, |r| r.perspective_scale, |r, v| r.perspective_scale = v),
    &plain("perspective_aspect", K_DEVELOP_ASPECT, ControlKind::Plain, |r| r.perspective_aspect, |r, v| r.perspective_aspect = v),
    &plain("perspective_x", K_DEVELOP_X_OFFSET, ControlKind::Plain, |r| r.perspective_x, |r, v| r.perspective_x = v),
    &plain("perspective_y", K_DEVELOP_Y_OFFSET, ControlKind::Plain, |r| r.perspective_y, |r, v| r.perspective_y = v),
];

/// Every table, in the panel's drawing order.
pub(crate) fn tables() -> [&'static [&'static dyn ControlWidget]; 21] {
    [TONE, PRESENCE, VIGNETTE, GRAIN, HDR_HEADROOM, HDR_SDR, LENS_MANUAL, LENS_PROFILE, LENS_CA, LENS_DEFRINGE, CAL_TINT, CAL_RED, CAL_GREEN, CAL_BLUE, WB_TINT, GRADE_GLOBALS, CROP, SHARPEN, NOISE_LUMA, NOISE_COLOR, TRANSFORM]
}

/// Resolve a control id to its implementation (the one data lookup).
pub(crate) fn find(control: &str) -> Option<&'static dyn ControlWidget> {
    tables()
        .iter()
        .flat_map(|table| table.iter())
        .copied()
        .find(|widget| widget.id() == control)
}

/// Apply one reported change through the registry: no variant, no match on the
/// control — the implementation is the behaviour.
pub(crate) fn apply(recipe: &mut EditRecipe, change: CapabilityChange) {
    if let Some(widget) = find(change.control) {
        widget.apply(recipe, change.value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_control_resolves_to_a_catalogue_band_and_reset() {
        for widget in tables().iter().flat_map(|table| table.iter()) {
            let ((lo, hi), _) = widget
                .band_and_reset()
                .unwrap_or_else(|| panic!("`{}` has no catalogue band", widget.id()));
            assert!(lo < hi, "`{}` band is empty: {lo}..{hi}", widget.id());
        }
    }

    #[test]
    fn every_control_id_appears_once() {
        let mut ids: Vec<&str> =
            tables().iter().flat_map(|table| table.iter()).map(|w| w.id()).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before, "a control is registered twice");
        assert_eq!(before, 68, "the registry's size is pinned; a new row changes it deliberately");
    }

    #[test]
    fn the_tables_are_the_known_controls_in_panel_order() {
        assert_eq!(TONE.iter().map(|w| w.id()).collect::<Vec<_>>(), ["exposure_ev", "contrast", "highlights", "shadows", "whites", "blacks"]);
        assert_eq!(PRESENCE.iter().map(|w| w.id()).collect::<Vec<_>>(), ["texture", "clarity", "dehaze", "vibrance", "saturation"]);
        assert_eq!(
            VIGNETTE.iter().map(|w| w.id()).collect::<Vec<_>>(),
            ["post_crop_vignette", "post_crop_vignette_mid", "post_crop_vignette_feather", "post_crop_vignette_round", "post_crop_vignette_style", "post_crop_vignette_hl"],
        );
        assert_eq!(GRAIN.iter().map(|w| w.id()).collect::<Vec<_>>(), ["grain", "grain_size", "grain_rough"]);
        assert_eq!(HDR_HEADROOM.iter().map(|w| w.id()).collect::<Vec<_>>(), ["hdr_max_ev"]);
        assert_eq!(
            HDR_SDR.iter().map(|w| w.id()).collect::<Vec<_>>(),
            ["sdr_blend", "sdr_brightness", "sdr_contrast", "sdr_highlights", "sdr_shadows", "sdr_whites", "sdr_clarity"],
        );
        assert_eq!(CROP.iter().map(|w| w.id()).collect::<Vec<_>>(), ["straighten_deg"]);
        assert_eq!(SHARPEN.iter().map(|w| w.id()).collect::<Vec<_>>(), ["sharpen_radius", "sharpen_detail", "sharpen_mask"]);
        assert_eq!(NOISE_LUMA.iter().map(|w| w.id()).collect::<Vec<_>>(), ["noise_reduction", "nr_detail", "nr_contrast"]);
        assert_eq!(NOISE_COLOR.iter().map(|w| w.id()).collect::<Vec<_>>(), ["color_nr", "color_nr_detail", "color_nr_smooth"]);
        assert_eq!(
            TRANSFORM.iter().map(|w| w.id()).collect::<Vec<_>>(),
            ["perspective_vertical", "perspective_horizontal", "perspective_rotate", "perspective_scale", "perspective_aspect", "perspective_x", "perspective_y"],
        );
        assert_eq!(LENS_MANUAL.iter().map(|w| w.id()).collect::<Vec<_>>(), ["lens_vignette", "lens_vignette_mid", "lens_distortion"]);
        assert_eq!(LENS_PROFILE.iter().map(|w| w.id()).collect::<Vec<_>>(), ["lens_profile_distortion_scale", "lens_profile_vignetting_scale"]);
        assert_eq!(LENS_CA.iter().map(|w| w.id()).collect::<Vec<_>>(), ["ca_r", "ca_b"]);
        assert_eq!(
            LENS_DEFRINGE.iter().map(|w| w.id()).collect::<Vec<_>>(),
            ["defringe_purple", "defringe_purple_lo", "defringe_purple_hi", "defringe_green", "defringe_green_lo", "defringe_green_hi"],
        );
        assert_eq!(CAL_TINT.iter().map(|w| w.id()).collect::<Vec<_>>(), ["cal_shadow_tint"]);
        assert_eq!(CAL_RED.iter().map(|w| w.id()).collect::<Vec<_>>(), ["cal_red_hue", "cal_red_sat"]);
        assert_eq!(CAL_GREEN.iter().map(|w| w.id()).collect::<Vec<_>>(), ["cal_green_hue", "cal_green_sat"]);
        assert_eq!(CAL_BLUE.iter().map(|w| w.id()).collect::<Vec<_>>(), ["cal_blue_hue", "cal_blue_sat"]);
        assert_eq!(WB_TINT.iter().map(|w| w.id()).collect::<Vec<_>>(), ["tint"]);
        assert_eq!(GRADE_GLOBALS.iter().map(|w| w.id()).collect::<Vec<_>>(), ["color_grade.blending", "color_grade.balance"]);
    }

    #[test]
    fn every_label_resolves_and_the_hinted_rows_carry_their_hint() {

        let hinted = |id: &str| {
            crate::controls::find(id).map(|w| w.hint().is_some()).unwrap_or(false)
        };
        for widget in tables().iter().flat_map(|table| table.iter()) {
            assert!(
                !autoshade::text::name(widget.label()).is_empty(),
                "`{}` has no label",
                widget.id()
            );
        }
        for id in ["lens_profile_distortion_scale", "lens_profile_vignetting_scale", "defringe_purple_lo", "defringe_purple_hi", "defringe_green_lo", "defringe_green_hi"] {
            assert!(hinted(id), "`{id}` should carry its disclosure line");
        }
        assert!(!hinted("exposure_ev"), "a plain row has no hint");
    }

    #[test]
    fn every_row_round_trips_through_read_and_apply() {
        let mut recipe = EditRecipe::default();
        for widget in tables().iter().flat_map(|table| table.iter()) {
            widget.apply(&mut recipe, 12.5);
            assert_eq!(widget.read(&recipe), 12.5, "`{}` does not round-trip", widget.id());
            let reset = widget.band_and_reset().unwrap().1;
            widget.apply(&mut recipe, reset);
            assert_eq!(widget.read(&recipe), reset);
        }
    }

    /// The two colours-grading rows are nested fields, not top-level recipe
    /// controls: their catalogue answer is the band `ColorGrade::clamp`
    /// enforces and the neutral the default recipe stores (blending 50).
    #[test]
    fn the_grading_rows_take_their_band_and_reset_from_the_catalogue() {
        assert_eq!(GRADE_GLOBALS[0].band_and_reset(), Some(((0.0, 100.0), 50.0)), "blending");
        assert_eq!(GRADE_GLOBALS[1].band_and_reset(), Some(((-100.0, 100.0), 0.0)), "balance");
    }

    #[test]
    fn companion_rows_apply_through_the_explicit_zero_encoding() {
        let mut recipe = EditRecipe::default();
        let widget = find("post_crop_vignette_mid").expect("companion row");
        assert_eq!(widget.read(&recipe), 50.0, "an absent companion reads its Lightroom default");
        widget.apply(&mut recipe, 80.0);
        assert_eq!(recipe.resolved("post_crop_vignette_mid"), 80.0);
        widget.apply(&mut recipe, 50.0);
        assert_eq!(recipe.resolved("post_crop_vignette_mid"), 50.0, "the default returns it to absent");
    }

    #[test]
    fn find_resolves_every_registered_id_and_nothing_else() {
        for widget in tables().iter().flat_map(|table| table.iter()) {
            assert!(find(widget.id()).is_some(), "`{}` is not resolvable", widget.id());
        }
        assert!(find("not-a-control").is_none());
    }

    /// OCP proof: the renderer was never written for this control. It is a new
    /// `impl ControlWidget` registered in an external table; the renderer draws
    /// it through the trait, the change is data, and the registry applies it
    /// without naming it.
    #[test]
    fn a_control_impl_the_renderer_never_named_is_drawn_and_applied() {
        use crate::panels::develop_forms::render_rows;
        struct Fake;
        impl ControlWidget for Fake {
            fn id(&self) -> &'static str {
                "exposure_ev"
            }
            fn label(&self) -> Key {
                K_DEVELOP_EXPOSURE
            }
            fn read(&self, recipe: &EditRecipe) -> f32 {
                recipe.exposure_ev
            }
            fn apply(&self, recipe: &mut EditRecipe, value: f32) {
                recipe.exposure_ev = value;
            }
            fn draw(
                &self,
                ui: &mut Ui,
                _lang: Lang,
                recipe: &EditRecipe,
                out: &mut Vec<CapabilityChange>,
            ) -> bool {
                let mut value = self.read(recipe);
                let dirty = slider(ui, Lang::En, "Fake", &mut value, -5.0, 5.0, 0.0);
                if dirty {
                    out.push(CapabilityChange { control: "exposure_ev", value });
                }
                dirty
            }
        }
        let table: &[&dyn ControlWidget] = &[&Fake];
        let mut recipe = EditRecipe::default();
        let mut out = Vec::new();
        let mut ctx = crate::harness::Ctx::new();
        let mut drew = false;
        ctx.run_default(|ctx| {
            crate::harness::central_panel(ctx, |ui| {
                render_rows(ui, Lang::En, &recipe, table, &mut out);
                drew = true;
            });
        });
        assert!(drew);
        // The view applies the reported change through the registry, which is
        // the trait's `apply`; nothing here knows the concrete control.
        super::apply(&mut recipe, CapabilityChange { control: "exposure_ev", value: 2.0 });
        assert_eq!(recipe.exposure_ev, 2.0);
    }
}
