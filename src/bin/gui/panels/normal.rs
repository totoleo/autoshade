//! Normal's sidebar (design-normal-mode.md §3, approved 2026-10-23): four
//! groups in the order a person asks for them — make it look good (a one-click
//! look), a little brighter or warmer (fine-tune), less noise, save it — and
//! nothing else. Drawn in the one workspace's controls column, between the
//! Portraits panel above and the shared panels below (`app.rs`
//! `controls_column`).
//!
//! Every verb here reuses a door the full workspace already has: a one-click
//! look is `produce_recipe` through `spawn_analyze` (a preset is a typed
//! direction, the reference folder is the library — Normal's own,
//! `library::NORMAL_NAME`), the four sliders write the recipe fields the
//! develop panel writes, the noise switch is the Retouch panel's AI denoise
//! (its ◈ card, shown on the canvas and saved as shown; off goes back to the
//! card it was made from), and the Save group is the single export and the
//! paste-then-render batch with a jpg's options, numbered rather than
//! replacing a jpg saved earlier. The words are plain (separate i18n keys,
//! design §6): none of the workspace's trade terms appear here.

use crate::*;
use crate::logic::look::{FineTune, OneClick};
#[cfg(test)]
use autoshade::pipeline::GradeRequest;

/// What the first AI denoise of a rendered image downloads: the SCUNet weight
/// plus its network file (`python/denoise.py` `WEIGHT_BYTES["color_real_psnr"]` +
/// `NETWORK_BYTES`, 71 994 286 bytes), in MB — pinned by a test against
/// that file.
pub(crate) const DENOISE_FIRST_DOWNLOAD_MB: u32 = 72;

/// The one line under the one-click group: what the last one-click did.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum NormalLine {
    #[default]
    Idle,
    Working,
    Done,
    /// No image key: the built-in look stood in (`advisor::HeuristicProposer`).
    BuiltIn,
    /// The look could not be made; the error's own text.
    Failed(String),
}

/// The JPEG quality steps of the Save group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SaveQuality {
    #[default]
    High,
    Medium,
    Small,
}

impl SaveQuality {
    pub(crate) const ALL: [SaveQuality; 3] = [SaveQuality::High, SaveQuality::Medium, SaveQuality::Small];

    pub(crate) fn label(self, lang: Lang) -> &'static str {
        match self {
            SaveQuality::High => tr(lang, K_NORMAL_HIGH),
            SaveQuality::Medium => tr(lang, K_NORMAL_MEDIUM),
            SaveQuality::Small => tr(lang, K_NORMAL_SMALL_FILE),
        }
    }

    pub(crate) fn jpeg_quality(self) -> u8 {
        match self {
            SaveQuality::High => 95,
            SaveQuality::Medium => 85,
            SaveQuality::Small => 70,
        }
    }
}

/// The noise switch's two cards for one photo: the card it was turned on
/// from, and the ◈ card the AI denoise made from it (`None` while it runs).
/// Ids, not indices: the strip can change under them in the full workspace.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DenoisePair {
    pub(crate) photo: PathBuf,
    pub(crate) plain: String,
    pub(crate) denoised: Option<String>,
}

/// Normal's own state. Nothing here is persisted: the remembered preferences
/// (the look strength among them) are the workspace's own.
#[derive(Default)]
pub(crate) struct NormalState {
    pub(crate) line: NormalLine,
    /// The reference folder picked this session.
    pub(crate) ref_dir: Option<PathBuf>,
    /// The folder is being read into Normal's library.
    pub(crate) building: bool,
    /// The photo the folder was picked for: the look the build hands over
    /// to is made for this photo only (`on_one_click_built`).
    pub(crate) build_photo: Option<PathBuf>,
    pub(crate) build_progress: Option<(usize, usize)>,
    /// The photo a one-click last landed on, and its four fine-tune values.
    pub(crate) result: Option<(PathBuf, FineTune)>,
    /// The noise switch's cards (`set_normal_denoise`); the switch itself
    /// reads the card on screen (`normal_denoise_on`).
    pub(crate) denoise_pair: Option<DenoisePair>,
    /// Whether the rendered-image denoiser's weight is on disk, read on the
    /// first frame the switch is drawn (`None` = not read yet).
    pub(crate) denoise_ready: Option<bool>,
    pub(crate) quality: SaveQuality,
    /// Is an image key set? Read on the first Normal frame and after a
    /// Settings save; `None` = not read yet.
    pub(crate) key_present: Option<bool>,
    /// 「Apply to every photo」 is between its paste and its render: the
    /// folder the jpgs go to, resolved before the paste.
    pub(crate) apply_all: Option<PathBuf>,
    /// A Save-group delivery is running (its landing is worded here).
    pub(crate) delivering: bool,
    /// Test seam: what the last analysis asked for (request, direction,
    /// library name).
    #[cfg(test)]
    pub(crate) last_request: Option<(GradeRequest, Option<String>, &'static str)>,
    /// Test seam: stands in for the paid chain inside the analyze worker.
    #[cfg(test)]
    pub(crate) runner: Option<AnalyzeRunner>,
    /// Test seam: the batch-folder dialog's answer (`Some(None)` = cancelled).
    #[cfg(test)]
    pub(crate) folder_pick: Option<Option<PathBuf>>,
    /// Test seam: the save window's answer (`Some(None)` = cancelled).
    #[cfg(test)]
    pub(crate) file_pick: Option<Option<PathBuf>>,
}

/// A stand-in for `produce_recipe` (tests only).
#[cfg(test)]
pub(crate) type AnalyzeRunner = fn(
    &std::path::Path,
    &GradeRequest,
) -> anyhow::Result<(EditRecipe, autoshade::advisor::Verdict, Vec<autoshade::rationale::Note>)>;

impl Commands<'_> {


    /// The sidebar: the four groups, behind the same mid-open gate as the
    /// shared panels (L15-2 — these controls write the open photo's recipe).
    pub(crate) fn normal_panel(&mut self, ui: &mut egui::Ui) {
        if self.workspace.normal().key_present.is_none() {
            self.workspace.normal_mut().key_present = Some(autoshade::config::Config::load().openai_api_key.is_some());
        }
        let editable = !self.workspace.open().open_in_flight;
        ui.enabled(editable, |ui| {
            self.normal_one_click(ui);
            self.normal_fine_tune(ui);
            self.normal_noise(ui);
            self.normal_save(ui);
        });
    }

    /// Group 1 — the one-click look: six fixed looks, or the reference folder.
    fn normal_one_click(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        ui.gap(SPACE_LG);
        panel_heading(ui, tr(lang, K_NORMAL_ONE_CLICK_LOOK), false, |_| {});
        let ready = self.editor.photo().src_path.is_some() && !self.workspace.busy() && !self.workspace.tools().analyze_inflight && !self.workspace.normal().building;
        let names: [&str; 6] = [
            tr(lang, K_NORMAL_NATURAL),
            tr(lang, K_NORMAL_BRIGHT_CLEAN),
            tr(lang, K_NORMAL_WARM),
            tr(lang, K_NORMAL_COOL_NIGHT),
            tr(lang, K_NORMAL_FILM),
            tr(lang, K_NORMAL_BLACK_WHITE),
        ];
        let presets = &autoshade::advisor::presets::PRESETS;
        let mut asked: Option<OneClick> = None;
        // Two to a row: at three, "Bright & clean" and "Black & white" are cut short.
        for row in 0..3 {
            ui.row(|ui| {
                let cell = columns(ui, 2);
                for col in 0..2 {
                    let i = row * 2 + col;
                    if action_in(ui, cell, ready, names[i])
                        .on_hover_text(tr(lang, K_NORMAL_THE_AI_LOOKS_AT_THIS_PHOTO_AND_GIVES_IT_THIS_LOO))
                        .clicked()
                    {
                        asked = Some(OneClick::Preset(presets[i].id));
                    }
                }
            });
        }
        ui.vertical_centered(|ui| ui.label(Text::new(tr(lang, K_NORMAL_OR)).weak().small()));
        let pick = action_row(ui, ready, tr(lang, K_NORMAL_MATCH_MY_REFERENCE_PHOTOS))
            .on_hover_text(tr(
                lang,
                K_NORMAL_PICK_A_FOLDER_OF_PHOTOS_WHOSE_LOOK_YOU_LIKE_THEY,
            ))
            .clicked();
        if let Some(dir) = &self.workspace.normal().ref_dir {
            let name = dir.file_name().map_or_else(|| abs_display(dir), |n| n.to_string_lossy().into_owned());
            ui.label(Text::new(trf(lang, K_NORMAL_CHOSEN, &[("name", &name)])).weak().small());
        }
        slider_pct_hinted(
            ui,
            lang,
            tr(lang, K_NORMAL_LOOK_STRENGTH),
            &mut self.aip.grade_strength,
            1.0,
            GRADE_STRENGTH_DEFAULT,
            tr(lang, K_NORMAL_HOW_STRONG_THE_LOOK_IS_DOUBLE_CLICK_TO_RESET),
        );
        let line = self.workspace.normal_line_text(lang);
        ui.row_wrapped(|ui| {
            if self.workspace.normal().building || (self.workspace.normal().line == NormalLine::Working && self.workspace.tools().analyze_inflight) {
                ui.spinner();
            }
            ui.label(Text::new(line).small());
        });
        if self.workspace.normal().key_present == Some(false) {
            ui.label(
                Text::new(tr(lang, K_NORMAL_TO_MAKE_LOOKS_WITH_AI_ADD_A_SERVICE_KEY_IN_SETTI))
                    .weak()
                    .small(),
            );
        }
        if let Some(ask) = asked {
            self.apply_actions(vec![logic::Action::Look(logic::look::LookIntent::of(
                logic::look::LookKind::OneClick(ask),
            ))]);
        }
        // The dialog after the group is laid out: it is modal and blocks, so
        // the runtime opens it and starts the build/look.
        if pick {
            self.apply_actions(vec![logic::Action::Normal(
                logic::normal::NormalAction::PickReferencesFolder,
            )]);
        }
    }

    /// Group 2 — fine-tune: four of the develop panel's fields under plain
    /// names, on top of the one-click result.
    fn normal_fine_tune(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        ui.gap(SPACE_LG);
        panel_heading(ui, tr(lang, K_NORMAL_FINE_TUNE), false, |_| {});
        let mut changed = false;
        let r = &mut self.editor.doc_mut().recipe;
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_BRIGHTNESS), &mut r.exposure_ev, -5.0, 5.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_CONTRAST), &mut r.contrast, -100.0, 100.0, 0.0);
        let mut warmth = r.warmth();
        if slider_hinted(
            ui,
            lang,
            tr(lang, K_NORMAL_WARMTH),
            &mut warmth,
            -100.0,
            100.0,
            0.0,
            tr(lang, K_NORMAL_COOLER_TO_THE_LEFT_WARMER_TO_THE_RIGHT_0_IS_THE),
        ) {
            let kelvin = r.kelvin_at_warmth(warmth);
            r.temperature_k = Some(kelvin);
            changed = true;
        }
        changed |= slider(ui, lang, tr(lang, K_NORMAL_VIVIDNESS), &mut r.saturation, -100.0, 100.0, 0.0);
        let back = self.workspace.normal()
            .result
            .as_ref()
            .filter(|(p, f)| self.editor.photo().src_path.as_ref() == Some(p) && *f != FineTune::of(&self.editor.doc().recipe))
            .map(|(_, f)| *f);
        if action_row(ui, back.is_some(), tr(lang, K_NORMAL_BACK_TO_THE_ONE_CLICK_RESULT)).clicked()
            && let Some(f) = back
        {
            f.apply(&mut self.editor.doc_mut().recipe);
            changed = true;
        }
        if changed {
            self.editor.doc_mut().recipe.clamp();
            self.editor.session_mut().dirty = true;
        }
    }

    /// Group 3 — noise: one switch, the Retouch panel's AI denoise
    /// (`set_normal_denoise`).
    fn normal_noise(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        ui.gap(SPACE_LG);
        panel_heading(ui, tr(lang, K_NORMAL_NOISE), false, |_| {});
        // A run that ended without its card (failed, cancelled) leaves no
        // pair behind.
        if !self.workspace.busy() && self.workspace.normal().denoise_pair.as_ref().is_some_and(|d| d.denoised.is_none()) {
            self.workspace.normal_mut().denoise_pair = None;
        }
        let helper = denoise_helper_available();
        let mut on = self.editor.normal_denoise_on(&self.workspace);
        let toggled = ui
            .add_enabled(helper && !self.workspace.busy(), egui::Checkbox::new(&mut on, tr(lang, K_NORMAL_AI_DENOISE)))
            .on_hover_text(tr(lang, K_NORMAL_CLEANS_THE_GRAIN_OUT_OF_THE_PHOTO_AND_THE_PREVIE))
            .changed();
        if helper && self.workspace.normal().denoise_ready.is_none() {
            let cfg = autoshade::config::Config::load();
            let weight = std::path::Path::new(&cfg.weights_dir).join(format!("scunet_{}.pth", cfg.denoise_model));
            self.workspace.normal_mut().denoise_ready = Some(weight.is_file());
        }
        if toggled {
            self.apply_actions(vec![logic::Action::Normal(
                logic::normal::NormalAction::SetDenoise(on),
            )]);
        }
        for line in self.editor.noise_lines(&self.workspace, lang, helper) {
            ui.label(Text::new(line).weak().small());
        }
    }


    /// Group 4 — save: a jpg of this photo, or the look on every photo of the
    /// folder.
    fn normal_save(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        ui.gap(SPACE_LG);
        panel_heading(ui, tr(lang, K_NORMAL_SAVE), false, |_| {});
        let ready = self.editor.photo().src_path.is_some() && !self.workspace.busy();
        let mut save = false;
        ui.row(|ui| {
            let cell = columns(ui, 2);
            save = primary_in(ui, cell, ready, tr(lang, K_NORMAL_SAVE_AS_JPG))
                .on_hover_text(tr(lang, K_NORMAL_SAVES_THE_PHOTO_WITH_THIS_LOOK_AS_A_JPG_THE_ORIG))
                .clicked();
            ui.menu("normal_quality", format!("{} · {}", tr(lang, K_NORMAL_QUALITY), self.workspace.normal().quality.label(lang)), Some(cell.x), |ui| {
                    for q in SaveQuality::ALL {
                        ui.selectable_value(&mut self.workspace.normal_mut().quality, q, q.label(lang));
                    }
                });
        });
        // The app's output folder from Settings, or the folder last saved to,
        // or a dialog — the one destination setting the workspace's Export
        // reads. (A folder inside the photo library is a valid destination; a
        // save only refuses to REPLACE an existing photo.)
        let place = match (self.workspace.exp().dest, self.workspace.export_dest_dir()) {
            (_, None) => tr(lang, K_NORMAL_ASKS_WHERE_TO_SAVE_EACH_TIME).to_string(),
            (ExportDest::LastUsed, Some(d)) => {
                trf(lang, K_NORMAL_SAVES_TO_THE_FOLDER_YOU_LAST_SAVED_TO, &[("folder", &crate::normal_actions::folder_name(&d))])
            }
            (_, Some(d)) => trf(
                lang,
                K_NORMAL_SAVES_TO_THE_APP_S_OUTPUT_FOLDER_CHANGE_IT_IN_SE,
                &[("folder", &crate::normal_actions::folder_name(&d))],
            ),
        };
        let full = self.workspace.export_dest_dir().map(|d| abs_display(&d)).unwrap_or_default();
        ui.label(Text::new(place).weak().small()).on_hover_text(full);
        let all = action_row(ui, ready && !self.workspace.gal().gallery.is_empty(), tr(lang, K_NORMAL_APPLY_THIS_LOOK_TO_EVERY_PHOTO_IN_THE_FOLDER))
            .on_hover_text(tr(lang, K_NORMAL_GIVES_EVERY_PHOTO_IN_THE_OPEN_FOLDER_THIS_LOOK_A))
            .on_disabled_hover_text(tr(lang, K_NORMAL_OPEN_A_FOLDER_FIRST_THE_LIBRARY_ON_THE_LEFT))
            .clicked();
        if save {
            self.apply_actions(vec![logic::Action::Normal(logic::normal::NormalAction::SaveJpg)]);
        }
        if all {
            self.apply_actions(vec![logic::Action::Normal(
                logic::normal::NormalAction::ApplyToFolder,
            )]);
        }
    }
}
