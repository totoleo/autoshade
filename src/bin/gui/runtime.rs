//! The facade between the view and the headless logic.
//!
//! The view draws a frame and returns [`Action`]s; this module hands them to
//! `logic::*`, then executes the [`Effect`]s that come back. Nothing in the
//! view mutates a recipe, spawns a worker or renders a logic-authored line —
//! those three crossings live here, on purpose.

use crate::logic::look::{AnalysisJob, LookIntent, LookKind, LookLine, OneClick};
use crate::logic::{self, Action, Effect};
use crate::*;

impl Commands<'_> {
    /// Apply what the view saw the user do. One arm per domain; each arm ends
    /// in `logic::<domain>`, and the effects run at the end, in the order the
    /// logic asked for them.
    pub(crate) fn apply_actions(&mut self, actions: Vec<Action>) {
        let mut effects = Vec::new();
        for action in actions {
            match action {
                Action::Look(intent) => self.start_look(intent),
                Action::References(dir) => {
                    let photo = self.editor.photo().src_path.clone();
                    let started = logic::look::references_start(
                        photo.as_deref(),
                        dir,
                        self.workspace.busy(),
                        self.workspace.tools().analyze_inflight,
                        self.workspace.normal().building,
                    );
                    self.run_effects(started);
                }
                Action::Lens(action) => {
                    let start = match action {
                        logic::lens_blur::LensAction::EstimateDepth => {
                            self.editor.photo().src_path.clone().zip(self.editor.original_frame_base()).map(|(photo, base)| (photo, base))
                        }
                        _ => None,
                    };
                    effects.extend(logic::lens_blur::apply(&mut self.editor.doc_mut().recipe, action, start, self.workspace.busy()));
                }
                Action::Gallery(action) => self.apply_gallery(action),
                Action::Pick(action) => {
                    effects.extend(logic::pick::apply(&mut self.editor.doc_mut().recipe, action));
                }
                Action::Crop(action) => {
                    effects.extend(logic::crop::apply(&mut self.editor.doc_mut().recipe, action));
                }
                Action::MaskEdit(action) => {
                    effects.extend(logic::mask_edit::apply(&mut self.editor.doc_mut().recipe, action));
                }
                Action::Region(action) => effects.extend(logic::region::apply(action)),
                Action::Portrait(action) => match action {
                    logic::portrait::PortraitAction::FindPeople => self.start_find_people(),
                    logic::portrait::PortraitAction::RemoveBlemishes => self.start_remove_blemishes(),
                    logic::portrait::PortraitAction::KeepPersonSharp(on) => {
                        if self.editor.set_keep_person_sharp(on) {
                            self.editor.session_mut().dirty = true;
                        }
                    }
                    logic::portrait::PortraitAction::SetKeepSkin(on) => self.editor.set_keep_skin(&mut self.workspace, self.lang, on),
                },
                Action::Develop(action) => match action {
                    logic::develop::DevelopAction::AiDenoise => self.start_ai_denoise(),
                    logic::develop::DevelopAction::Stack(extra) => self.start_stack(extra),
                    logic::develop::DevelopAction::MaskBrush(target) => self.start_mask_brush(target),
                    logic::develop::DevelopAction::Segment { target, label } => {
                        self.start_segment(target, label)
                    }
                    logic::develop::DevelopAction::MaskRefine(i) => self.start_mask_refine(i),
                    logic::develop::DevelopAction::MoveMaskUp(i) => self.editor.move_mask(&mut self.workspace, i, true),
                    logic::develop::DevelopAction::MoveMaskDown(i) => self.editor.move_mask(&mut self.workspace, i, false),
                    logic::develop::DevelopAction::DuplicateMask(i) => self.duplicate_mask(self.lang, i),
                    logic::develop::DevelopAction::LoadVersion(n) => self.load_version(n),
                    logic::develop::DevelopAction::DeleteVersion(n) => {
                        if let Some(src) = self.editor.photo().src_path.clone() {
                            self.delete_version_clicked(self.lang, src, n);
                        }
                    }
                },
                Action::Recipe(action) => match action {
                    logic::recipes::RecipeAction::ImportFile => self.import_recipe(),
                    logic::recipes::RecipeAction::Refresh => self.workspace.refresh_recipes(),
                    logic::recipes::RecipeAction::SaveCurrent => self.save_current_recipe(),
                    logic::recipes::RecipeAction::ApplyToOpen(name) => self.apply_recipe_to_open(&name),
                    logic::recipes::RecipeAction::ApplyToSelection(name) => {
                        self.apply_recipe_to_selection(&name)
                    }
                    logic::recipes::RecipeAction::UpdateFromOpen(name) => {
                        self.update_recipe_from_open(&name)
                    }
                    logic::recipes::RecipeAction::Rename { from, to } => self.rename_recipe(&from, &to),
                    logic::recipes::RecipeAction::Delete(name) => self.delete_recipe(&name),
                },
                Action::Ai(action) => match action {
                    logic::ai::AiAction::PickRawFolder => {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            self.aip.style_raw_dir = Some(dir);
                        }
                    }
                    logic::ai::AiAction::RefreshLibraryStatus => {
                        // Not in tests: a headless frame would spawn a worker
                        // reading the developer's own store.
                        if !cfg!(test) && self.workspace.ai().style_info.is_none() && !self.workspace.ai().style_info_loading {
                            self.start_style_info();
                        }
                    }
                    logic::ai::AiAction::BuildReferenceLibrary => {
                        if let Some(dir) = self.aip.style_src_dir.clone().filter(|d| d.is_dir()) {
                            self.start_style_build(dir);
                        }
                    }
                    logic::ai::AiAction::Reimagine => self.start_reimagine(),
                    logic::ai::AiAction::AdjustGenerated => self.start_adjust(),
                    logic::ai::AiAction::ReverseFit => self.start_fit(),
                    logic::ai::AiAction::StylePrompt => self.start_style_prompt(),
                    logic::ai::AiAction::PickReferenceFolder => {
                        let mut dialog = rfd::FileDialog::new();
                        // Reopen where the last build ran, when it still exists.
                        if let Some(d) = self.aip.style_src_dir.clone().filter(|d| d.is_dir()) {
                            dialog = dialog.set_directory(d);
                        }
                        if let Some(dir) = dialog.pick_folder() {
                            // Remembered on the PICK: a build that then fails must
                            // not lose the folder the rebuild button needs.
                            self.aip.style_src_dir = Some(dir.clone());
                            self.start_style_build(dir);
                        }
                    }
                },
                Action::Normal(action) => match action {
                    logic::normal::NormalAction::SaveJpg => self.start_normal_save(),
                    logic::normal::NormalAction::ApplyToFolder => self.start_apply_to_folder(),
                    logic::normal::NormalAction::SetDenoise(on) => self.set_normal_denoise(on),
                    logic::normal::NormalAction::PickReferencesFolder => {
                        let mut dialog = rfd::FileDialog::new();
                        if let Some(d) = self.workspace.normal().ref_dir.clone().filter(|d| d.is_dir()) {
                            dialog = dialog.set_directory(d);
                        }
                        if let Some(dir) = dialog.pick_folder() {
                            // The known folder goes through the same landing
                            // the view's own `References` action uses.
                            self.apply_actions(vec![Action::References(dir)]);
                        }
                    }
                },
                Action::Settings(action) => match action {
                    logic::settings::SettingsAction::ImportLegacyFolder => {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            self.start_import_legacy(dir);
                        }
                    }
                    logic::settings::SettingsAction::PickDeliveryFolder => {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            self.workspace.ui_mut().settings.out_dir = dir.display().to_string();
                        }
                    }
                    logic::settings::SettingsAction::SaveForm => self.save_settings_form(),
                    logic::settings::SettingsAction::FetchModels(side) => {
                        let role = match side {
                            logic::settings::ModelSide::Analysis => ModelRole::Analysis,
                            logic::settings::ModelSide::Image => ModelRole::Image,
                        };
                        self.fetch_models(role);
                    }
                    logic::settings::SettingsAction::InstallPython => self.start_python_env_install(),
                },
            }
        }
        self.run_effects(effects);
    }

    /// Execute the effects the logic asked for: worker spawns, lines, flags.
    pub(crate) fn run_effects(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::EstimateDepth { photo, base } => self.spawn_depth_worker(photo, base),
                Effect::Status(text) => *self.workspace.status_mut() = crate::i18n::text(self.lang, &text),
                Effect::ToastError(text) => {
                    self.workspace.toast(ToastKind::Error, crate::i18n::text(self.lang, &text));
                }
                Effect::LookLine { kind, line } => self.apply_look_line(kind, line),
                Effect::RememberOneClick { photo, fine } => {
                    self.workspace.normal_mut().result = Some((photo, fine));
                }
                Effect::StartLook(intent) => self.start_look(intent),
                Effect::ReferenceBuild { photo, dir } => self.start_reference_build(photo, dir),
                Effect::ArmFocusPick(on) => self.workspace.tools_mut().lens_focus_picking = on,
                Effect::SetRegion(region) => self.workspace.overlay_mut().region = region,
                Effect::DisarmPicker(picker) => match picker {
                    logic::pick::Picker::WhiteBalance => self.workspace.tools_mut().wb_picking = false,
                    logic::pick::Picker::Range => self.workspace.tools_mut().range_picking = None,
                    logic::pick::Picker::PointColor => self.workspace.tools_mut().point_color_picking = false,
                },
                Effect::DisarmTools => self.workspace.disarm_tools(&mut self.editor),
                Effect::MarkDirty => self.editor.session_mut().dirty = true,
            }
        }
    }

    /// `Msg::DepthEstimated`'s landing: the wire is free either way, so the
    /// in-flight flag clears here; the recipe change and its words are the
    /// logic's.
    pub(crate) fn on_depth_estimated(&mut self, res: anyhow::Result<std::path::PathBuf>) {
        self.workspace.set_busy(false);
        let effects = logic::lens_blur::landed(&mut self.editor.doc_mut().recipe, res);
        self.run_effects(effects);
    }




    /// Read the picked folder into Normal's reference library, for the photo
    /// that was open then. Progress and the built hand-off arrive as messages;
    /// the decisions are the logic layer's.
    fn start_reference_build(&mut self, photo: std::path::PathBuf, dir: std::path::PathBuf) {
        self.workspace.set_busy(true);
        self.workspace.normal_mut().build_photo = Some(photo);
        self.workspace.normal_mut().building = true;
        self.workspace.normal_mut().build_progress = None;
        self.workspace.normal_mut().ref_dir = Some(dir.clone());
        let tx = self.services.tx.clone();
        let ctx = self.services.view_ctx.clone();
        self.services.spawn_worker(
            move || {
                let progress = |p: autoshade::library::BuildProgress| {
                    let _ = tx.send(Msg::OneClickBuildProgress { done: p.done, total: p.total });
                    ctx.request_repaint();
                };
                let sidecars = autoshade::library::Sidecars::from_config(&autoshade::config::Config::load());
                let built = autoshade::library::rebuild_named(
                    autoshade::library::NORMAL_NAME,
                    &dir,
                    &sidecars,
                    autoshade::library::DescribeSwitch::OFF,
                    None,
                    &progress,
                );
                Msg::OneClickBuilt(built.map(|lib| lib.entries.len()).map_err(|e| format!("{e:#}")))
            },
            |e| Msg::OneClickBuilt(Err(e.to_string())),
        );
    }

    /// `Msg::OneClickBuilt`'s landing: free the wire, then let the logic decide
    /// whether this build hands over to the reference look for the photo it was
    /// started for.
    pub(crate) fn on_one_click_built(&mut self, _lang: Lang, res: Result<usize, String>) {
        self.workspace.set_busy(false);
        self.workspace.normal_mut().building = false;
        self.workspace.normal_mut().build_progress = None;
        let for_photo = self.workspace.normal_mut().build_photo.take();
        let effects = logic::look::references_built(
            &res,
            for_photo.as_deref(),
            self.editor.photo().src_path.as_deref(),
            self.workspace.tools().analyze_inflight,
        );
        self.run_effects(effects);
    }

    /// Plan and start one AI look: snapshot the panel prefs on this thread, let
    /// `logic::look::plan` decide, store the pending look and run the worker.
    fn start_look(&mut self, intent: LookIntent) {
        let prefs = logic::look::LookPrefs {
            guide: self.aip.guide,
            style: self.aip.style_strength,
            send_reference_image: self.aip.send_style_ref_image,
            strength: self.aip.analysis_strength(),
            think: self.aip.deep_think,
            adherence: autoshade::recipe::DirectionAdherence::new(self.aip.direction_adherence),
            embed: autoshade::library::EmbeddingSwitch::resolve(None, self.aip.style_embed),
        };
        let photo = self.editor.photo().src_path.clone();
        let Some(started) = logic::look::plan(
            &prefs,
            intent,
            photo.as_deref(),
            self.workspace.busy(),
            self.workspace.tools().analyze_inflight,
            self.workspace.tools().gen_epoch,
        ) else {
            return;
        };
        self.workspace.tools_mut().look.pending = Some(started.pending);
        self.run_effects(started.effects);
        self.spawn_analysis(started.job);
    }

    /// Run one planned analysis. The job's epoch is the one `plan` read from
    /// the app when the verb was pressed; the worker arms the cancel for it.
    fn spawn_analysis(&mut self, job: AnalysisJob) {
        debug_assert_eq!(job.epoch, self.workspace.tools().gen_epoch, "a planned job starts under the current epoch");
        self.workspace.set_busy(true);
        self.spawn_analyze(job.photo, job.pixels, job.guidance, job.base, job.req, job.library);
    }


    /// Apply one gallery action: the folder dialog, the open/toggle decision,
    /// the thumbnail worker and the bounded texture window are the runtime's.
    fn apply_gallery(&mut self, action: logic::gallery::GalleryAction) {
        use logic::gallery::{Click, GalleryAction};
        match action {
            GalleryAction::OpenFolder => {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.open_folder(dir);
                }
            }
            GalleryAction::OpenPhotoFile => {
                if let Some(p) = open_photo_dialog() {
                    self.open_path(p);
                }
            }
            GalleryAction::Click { index, multi } => match logic::gallery::click(index, multi) {
                Click::Open(index) => self.open_gallery_index(index),
                Click::Toggle(index) => logic::gallery::toggle(&mut self.workspace.gal_mut().multi_sel, index),
            },
            GalleryAction::CopyRecipe => {
                // The clipboard must carry the name the user sees in the box.
                self.commit_pending_names();
                self.workspace.genai_mut().copied = Some(self.editor.doc().recipe.clone());
                self.workspace.genai_mut().copied_from = self.editor.photo().src_path.clone();
                *self.workspace.status_mut() = crate::i18n::tr(
                    self.lang,
                    K_APP_RECIPE_COPIED_CTRL_CLICK_TO_PICK_SEVERAL_THEN_PA,
                )
                .to_owned();
            }
            GalleryAction::PasteToSelected => self.start_paste(),
            GalleryAction::RenderSelected => self.start_batch_render(),
            GalleryAction::OpenRecipeLibrary => self.open_recipe_library(),
            GalleryAction::ClearSelection => self.workspace.gal_mut().multi_sel.clear(),
            GalleryAction::ThumbNeeded(index) => self.request_thumb(index),
            GalleryAction::Trim { start, end } => {
                self.workspace.gal_mut().thumbs.retain(|i, _| (*i >= start) && (*i < end));
                self.workspace.gal_mut().thumb_requested.retain(|i| (*i >= start) && (*i < end));
            }
        }
    }

    /// Map a look's presentation line onto the panel that owns it. The wording
    /// for a failure is the status the shared landing already wrote; the line
    /// only carries the fact.
    fn apply_look_line(&mut self, kind: LookKind, line: LookLine) {
        match kind {
            LookKind::OneClick(OneClick::Preset(_)) | LookKind::OneClick(OneClick::References) => {
                self.workspace.normal_mut().line = match line {
                    LookLine::Idle => crate::panels::normal::NormalLine::Idle,
                    LookLine::Working => crate::panels::normal::NormalLine::Working,
                    LookLine::Done => crate::panels::normal::NormalLine::Done,
                    LookLine::BuiltIn => {
                        self.workspace.normal_mut().key_present = Some(false);
                        crate::panels::normal::NormalLine::BuiltIn
                    }
                    LookLine::Failed => crate::panels::normal::NormalLine::Failed(self.workspace.status().clone()),
                    LookLine::Looked => return,
                };
                // A landed one-click says its own plain sentence instead of the
                // shared landing's (and in the toast that landing raised).
                if matches!(line, LookLine::Done | LookLine::BuiltIn) {
                    let plain = self.workspace.normal_line_text(self.lang).to_string();
                    self.workspace.reword_landing(plain);
                }
            }
            LookKind::Portrait => {
                self.workspace.portrait_mut().line = match line {
                    LookLine::Idle => crate::panels::portrait::PortraitLine::Idle,
                    LookLine::Working => crate::panels::portrait::PortraitLine::Looking,
                    LookLine::Looked => crate::panels::portrait::PortraitLine::Looked,
                    LookLine::Failed => crate::panels::portrait::PortraitLine::Failed(self.workspace.status().clone()),
                    LookLine::Done | LookLine::BuiltIn => return,
                };
            }
            LookKind::Analyze | LookKind::Adjust => {}
        }
    }



    fn spawn_depth_worker(
        &mut self,
        photo: std::path::PathBuf,
        base: std::sync::Arc<image::DynamicImage>,
    ) {
        self.workspace.set_busy(true);
        self.services.spawn_worker(
            move || Msg::DepthEstimated(logic::lens_blur::estimate_depth(&photo, &base)),
            |e| Msg::DepthEstimated(Err(e)),
        );
    }
}
