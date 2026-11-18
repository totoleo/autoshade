//! The Portraits panel (design-portraits-mode.md §3, the user's ruling of
//! 2026-10-23: the full workspace, and one "Portraits" panel at the top of
//! the sidebar). Drawn first in the one workspace's controls column
//! (`app.rs` `controls_column`), above Normal's groups and the shared panels.
//!
//! Four groups, in the order a portrait is worked: find the people and the
//! faces (and an AI look made for portraits), the skin, the eyes, the
//! background. The finding is two sidecars on the original-frame preview —
//! the subject matte (`segment.py --target subject`) and the face finder
//! (`face.py`, MediaPipe) — and lands four ordinary bitmap masks carrying
//! their `MaskRole` (`autoshade::face`): Face, Eyes, Skin (the face narrowed
//! by a colour range seeded from the face) and Background (the subject,
//! inverted). Every slider here writes a field of the mask with that role,
//! or of the lens blur, so the recipe format does not change and every one
//! of them is also in the mask list below, editable there.
//!
//! "Keep skin natural" (§4) is a switch of this panel, on by default, not a
//! recipe field. While it is on, every write of the recipe — a slider here or
//! in the mask list, a paste, an AI landing — meets the same limits at one
//! place (`guard_portrait_skin`, run once per frame after the panels drew and
//! the workers landed). A write that goes to DISK in the same step — an AI
//! landing's save, a paste onto other photos — meets them where it is written
//! (`skin_held` / `skin_held_for`), before the file.

use crate::*;
use autoshade::face::{
    PORTRAIT_ROLES, crisp_of, darken_of, face_raster, fade_of, role_mask, set_crisp, set_darken,
    set_fade, set_soften, skin_target, soften_of, subject_raster,
};
use autoshade::recipe::{LocalAdjustment, MaskRole};

/// The eyes' "Brighten" slider's reach, in stops.
const EYES_BRIGHTEN_MAX_EV: f32 = 1.5;

pub(crate) use crate::logic::portrait::PortraitFind;




/// The one line under the first group.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum PortraitLine {
    #[default]
    Idle,
    Finding,
    /// The finding landed: faces found, and what failed, in the error's
    /// own words (the subject matte or the face finder).
    Found { faces: usize, failed: Option<String> },
    /// Nothing could be found at all.
    Failed(String),
    Looking,
    Looked,
}

/// The Portraits panel's own state. Nothing here is persisted (the
/// preferences file is unchanged); the masks it makes live in the recipe.
pub(crate) struct PortraitState {
    pub(crate) line: PortraitLine,
    /// The photo the last find step ran on, and how many faces it found —
    /// which words the greyed groups use ("no face found" vs "find them
    /// first").
    pub(crate) found: Option<(PathBuf, usize)>,
    /// "Keep skin natural" (design §4), per photo and card: (photo, card
    /// id) → on; a card with no entry is on. Opening never changes a value:
    /// when a photo or one of its saved versions is read from disk, every
    /// card whose saved edit already goes past the limits is entered OFF
    /// (`decide_keep_skin_for_photo` / `_for_card`), and nothing is clamped
    /// there until the user turns it on (`set_keep_skin`). A card made in
    /// this session (an AI look, a fit, a fork's default) starts on, so the
    /// app's own writes meet the limits; a fork inherits its parent's switch
    /// (`inherit_keep_skin`).
    pub(crate) keep_skin: std::collections::HashMap<(PathBuf, String), bool>,
    /// A depth estimate was started from the Background group: its landing
    /// aims the focus at the face and keeps the person sharp.
    pub(crate) blur_from_panel: bool,
    /// Test seam: what the last heal was started with (painted mask?, the
    /// limit raster) — `start_heal`, shared by the Retouch panel's heal and
    /// this panel's 🤖 Remove blemishes.
    #[cfg(test)]
    pub(crate) heal_asked: Option<(bool, Option<PathBuf>)>,
    /// Test seam: stands in for `retouch::heal` inside the heal worker.
    #[cfg(test)]
    pub(crate) heal_runner: Option<HealRunner>,
}

/// What the heal worker runs: `retouch::heal`, or a test's stand-in for it
/// — (the source, the limit raster, the output).
pub(crate) type HealRunner =
    fn(&std::path::Path, Option<&std::path::Path>, &std::path::Path) -> anyhow::Result<autoshade::retouch::HealReport>;

impl Default for PortraitState {
    fn default() -> Self {
        PortraitState {
            line: PortraitLine::Idle,
            found: None,
            keep_skin: Default::default(),
            blur_from_panel: false,
            #[cfg(test)]
            heal_asked: None,
            #[cfg(test)]
            heal_runner: None,
        }
    }
}

/// The find step itself, off the UI thread: the subject matte and the faces
/// on the original-frame preview (`original`), every raster under a freshly
/// claimed name beside the photo's develop. The skin seed is sampled from the
/// canvas's `base` as `recipe` develops it up to the skin mask
/// ([`panel_skin_seed`]); `lens` is the composed profile when that base is
/// lens-corrected (the two are then different frames). `film` / `raw_source`
/// are the canvas's (`film_short_edge`, `source_is_raw`).
#[allow(clippy::too_many_arguments)] // the two pictures, the base's lens, the photo, and the canvas's recipe, film edge and source kind
impl Commands<'_> {
    /// 🤖 Find people and faces: both sidecars in one worker.
        /// The find landing: the four masks into the recipe — a mask
    /// that already has its role is re-pointed at the new raster and keeps
    /// its sliders — and the line says what was found.
        /// ✨ AI portrait look: the portrait preset through the one analysis door,
    /// a fresh look at the look strength. The masks and blur the photo has now
    /// are remembered by the look protocol and put back on the landed recipe.
    pub(crate) fn start_portrait_look(&mut self) {
        let kept: Vec<LocalAdjustment> =
            self.editor.doc().recipe.masks.iter().filter(|m| PORTRAIT_ROLES.contains(&m.role)).cloned().collect();
        let mut intent = logic::look::LookIntent::of(logic::look::LookKind::Portrait);
        intent.kept_masks = kept;
        intent.kept_blur = self.editor.doc().recipe.lens_blur.clone();
        self.apply_actions(vec![logic::Action::Look(intent)]);
    }



    /// The panel: a first-level heading and the four groups, behind the same
    /// mid-open gate as the shared panels.
    pub(crate) fn portrait_panel(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        let editable = !self.workspace.open().open_in_flight;
        ui.gap(SPACE_LG);
        panel_heading(ui, tr(lang, K_PORTRAIT_PORTRAITS), false, |_| {});
        let mut changed = false;
        ui.enabled(editable, |ui| {
            self.portrait_people(ui);
            changed |= self.portrait_skin(ui);
            changed |= self.portrait_eyes(ui);
            changed |= self.portrait_background(ui);
        });
        if changed {
            self.editor.doc_mut().recipe.clamp();
            self.editor.session_mut().dirty = true;
        }
        ui.gap(SPACE_MD);
        ui.rule();
    }


    /// Group 1 — the people and the background, and the AI portrait look.
    fn portrait_people(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        portrait_group(ui, tr(lang, K_PORTRAIT_PEOPLE_AND_BACKGROUND));
        let ready = self.editor.photo().src_path.is_some() && self.editor.photo().base_preview.is_some() && !self.workspace.busy() && !self.workspace.tools().analyze_inflight;
        let mut find = false;
        let mut look = false;
        ui.row(|ui| {
            let cell = columns(ui, 2);
            find = action_in(ui, cell, ready, tr(lang, K_PORTRAIT_FIND_PEOPLE_AND_FACES))
                .on_hover_text(tr(lang, K_PORTRAIT_FINDS_THE_PERSON_AND_EVERY_FACE_ON_THIS_COMPUTER))
                .clicked();
            look = action_in(ui, cell, ready, tr(lang, K_PORTRAIT_AI_PORTRAIT_LOOK))
                .on_hover_text(tr(lang, K_PORTRAIT_THE_AI_LOOKS_AT_THIS_PHOTO_AND_GIVES_IT_A_PORTRA))
                .clicked();
        });
        slider_pct_hinted(
            ui,
            lang,
            tr(lang, K_NORMAL_LOOK_STRENGTH),
            &mut self.aip.grade_strength,
            1.0,
            GRADE_STRENGTH_DEFAULT,
            tr(lang, K_NORMAL_HOW_STRONG_THE_LOOK_IS_DOUBLE_CLICK_TO_RESET),
        );
        let line = self.workspace.portrait_line_text(lang);
        ui.row_wrapped(|ui| {
            if self.workspace.portrait().line == PortraitLine::Finding || (self.workspace.portrait().line == PortraitLine::Looking && self.workspace.tools().analyze_inflight) {
                ui.spinner();
            }
            ui.label(Text::new(line).small());
        });
        if find {
            self.apply_actions(vec![logic::Action::Portrait(
                logic::portrait::PortraitAction::FindPeople,
            )]);
        }
        if look {
            self.start_portrait_look();
        }
    }

    /// Group 2 — the skin: soften, warmth, colour, the guardrail switch.
    fn portrait_skin(&mut self, ui: &mut Ui) -> bool {
        let lang = self.lang;
        portrait_group(ui, tr(lang, K_PORTRAIT_SKIN));
        let gate = self.editor.portrait_face_gate(&self.workspace, lang);
        let guarded = self.editor.keep_skin_on(&self.workspace);
        let mut changed = false;
        ui.enabled(gate.is_none(), |ui| {
            let mut probe = LocalAdjustment::default();
            let m = skin_target(&mut self.editor.doc_mut().recipe).unwrap_or(&mut probe);
            let mut soften = soften_of(m);
            let soft_max = logic::portrait::reach(guarded, -autoshade::face::SKIN_TEXTURE_MIN, 100.0);
            if slider_hinted(ui, lang, tr(lang, K_PORTRAIT_SOFTEN_SKIN), &mut soften, 0.0, soft_max, 0.0, tr(lang, K_PORTRAIT_SMOOTHS_THE_SKIN_S_FINE_TEXTURE_THE_FACE_S_SHAPE)) {
                set_soften(m, soften);
                changed = true;
            }
            let warm = logic::portrait::reach(guarded, autoshade::face::SKIN_TEMPERATURE_MAX, 100.0);
            changed |= slider_hinted(ui, lang, tr(lang, K_PORTRAIT_SKIN_WARMTH), &mut m.temperature, -warm, warm, 0.0, tr(lang, K_PORTRAIT_COOLER_TO_THE_LEFT_WARMER_TO_THE_RIGHT_ON_THE_SK));
            let rich = logic::portrait::reach(guarded, autoshade::face::SKIN_SATURATION_MAX, 100.0);
            changed |= slider_hinted(ui, lang, tr(lang, K_PORTRAIT_SKIN_COLOUR_INTENSITY), &mut m.saturation, -100.0, rich, 0.0, tr(lang, K_PORTRAIT_PALER_TO_THE_LEFT_RICHER_TO_THE_RIGHT_ON_THE_SKI));
        });
        let mut on = guarded;
        let flipped = ui
            .checkbox(&mut on, tr(lang, K_PORTRAIT_KEEP_SKIN_NATURAL))
            .on_hover_text(tr(
                lang,
                K_PORTRAIT_ON_SKIN_COLOUR_CAN_BE_MADE_ONLY_A_LITTLE_RICHER,
            ))
            .changed();
        if flipped {
            self.apply_actions(vec![logic::Action::Portrait(
                logic::portrait::PortraitAction::SetKeepSkin(on),
            )]);
        }
        let can_heal = face_raster(&self.editor.doc().recipe).is_some() && !self.workspace.busy() && self.editor.photo().src_path.is_some();
        if action_row(ui, can_heal, tr(lang, K_PORTRAIT_REMOVE_BLEMISHES))
            .on_hover_text(tr(
                lang,
                K_PORTRAIT_THE_AI_FINDS_SMALL_SPOTS_AND_BLEMISHES_ON_THE_FA,
            ))
            .clicked()
        {
            self.apply_actions(vec![logic::Action::Portrait(
                logic::portrait::PortraitAction::RemoveBlemishes,
            )]);
        }
        if let Some(why) = gate {
            ui.label(Text::new(why).weak().small());
        }
        changed
    }

    /// Group 3 — the eyes: brighten, crisp.
    fn portrait_eyes(&mut self, ui: &mut Ui) -> bool {
        let lang = self.lang;
        portrait_group(ui, tr(lang, K_PORTRAIT_EYES));
        let gate = self.editor.portrait_face_gate(&self.workspace, lang).or_else(|| {
            (!self.editor.doc().recipe.masks.iter().any(|m| m.role == MaskRole::Eyes)).then(|| tr(lang, K_PORTRAIT_FIND_THE_PEOPLE_AND_FACES_FIRST_THE_FIRST_GROUP))
        });
        let mut changed = false;
        ui.enabled(gate.is_none(), |ui| {
            let mut probe = LocalAdjustment::default();
            let m = role_mask(&mut self.editor.doc_mut().recipe, MaskRole::Eyes).unwrap_or(&mut probe);
            changed |= slider_hinted(ui, lang, tr(lang, K_PORTRAIT_BRIGHTEN_EYES), &mut m.exposure_ev, 0.0, EYES_BRIGHTEN_MAX_EV, 0.0, tr(lang, K_PORTRAIT_LIFTS_THE_EYES_A_LITTLE));
            let mut crisp = crisp_of(m);
            if slider_hinted(ui, lang, tr(lang, K_PORTRAIT_CRISP_EYES), &mut crisp, 0.0, 100.0, 0.0, tr(lang, K_PORTRAIT_BRINGS_OUT_THE_DETAIL_IN_THE_EYES)) {
                set_crisp(m, crisp);
                changed = true;
            }
        });
        if let Some(why) = gate {
            ui.label(Text::new(why).weak().small());
        }
        changed
    }

    /// Group 4 — the background: the blur (with the person kept sharp), and
    /// darken / fade on the background mask.
    fn portrait_background(&mut self, ui: &mut Ui) -> bool {
        let lang = self.lang;
        portrait_group(ui, tr(lang, K_PORTRAIT_BACKGROUND));
        let mut changed = false;
        let subject = subject_raster(&self.editor.doc().recipe);
        if self.editor.doc().recipe.lens_blur.is_none() {
            let can = !self.workspace.busy() && self.editor.photo().base_preview.is_some() && self.editor.photo().src_path.is_some() && depth_helper_available();
            if action_row(ui, can, tr(lang, K_PORTRAIT_PREPARE_THE_BACKGROUND_BLUR))
                .on_hover_text(tr(lang, K_PORTRAIT_MEASURES_HOW_FAR_AWAY_EVERY_PART_OF_THE_PHOTO_IS))
                .clicked()
            {
                self.workspace.portrait_mut().blur_from_panel = true;
                self.apply_actions(vec![logic::Action::Lens(logic::lens_blur::LensAction::EstimateDepth)]);
            }
        } else if let Some(lb) = self.editor.doc_mut().recipe.lens_blur.as_mut() {
            changed |= slider(ui, lang, tr(lang, K_LENS_BLUR_AMOUNT), &mut lb.amount, 0.0, 100.0, 50.0);
            changed |= slider_hinted(ui, lang, tr(lang, K_PORTRAIT_SHARP_RANGE), &mut lb.range, 0.0, 100.0, 10.0, tr(lang, K_PORTRAIT_HOW_DEEP_THE_SHARP_PART_OF_THE_PICTURE_IS_AROUND));
            let mut keep = lb.protect.is_some();
            let toggled = ui
                .add_enabled(subject.is_some() || keep, Checkbox::new(&mut keep, tr(lang, K_PORTRAIT_KEEP_THE_PERSON_SHARP)))
                .on_hover_text(tr(lang, K_PORTRAIT_THE_WHOLE_PERSON_STAYS_SHARP_HOWEVER_FAR_THE_BLU))
                .on_disabled_hover_text(tr(lang, K_PORTRAIT_FIND_THE_PEOPLE_AND_FACES_FIRST_THE_FIRST_GROUP))
                .changed();
            if toggled {
                self.apply_actions(vec![logic::Action::Portrait(
                    logic::portrait::PortraitAction::KeepPersonSharp(keep),
                )]);
            }
        }
        let has_bg = self.editor.doc().recipe.masks.iter().any(|m| m.role == MaskRole::Background);
        ui.enabled(has_bg, |ui| {
            let mut probe = LocalAdjustment::default();
            let m = role_mask(&mut self.editor.doc_mut().recipe, MaskRole::Background).unwrap_or(&mut probe);
            let mut darken = darken_of(m);
            if slider_hinted(ui, lang, tr(lang, K_PORTRAIT_DARKEN_BACKGROUND), &mut darken, 0.0, 100.0, 0.0, tr(lang, K_PORTRAIT_EVERYTHING_BUT_THE_PERSON_A_LITTLE_DARKER)) {
                set_darken(m, darken);
                changed = true;
            }
            let mut fade = fade_of(m);
            if slider_hinted(ui, lang, tr(lang, K_PORTRAIT_FADE_BACKGROUND_COLOUR), &mut fade, 0.0, 100.0, 0.0, tr(lang, K_PORTRAIT_EVERYTHING_BUT_THE_PERSON_A_LITTLE_GREYER)) {
                set_fade(m, fade);
                changed = true;
            }
        });
        if !has_bg {
            ui.label(Text::new(tr(lang, K_PORTRAIT_FIND_THE_PEOPLE_AND_FACES_FIRST_THE_FIRST_GROUP)).weak().small());
        }
        changed
    }
}

/// A group's sub-heading.
pub(crate) fn portrait_group(ui: &mut Ui, title: &str) {
    ui.gap(SPACE_MD);
    ui.label(Text::new(title).strong());
}
