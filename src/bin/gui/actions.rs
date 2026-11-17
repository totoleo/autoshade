//! App actions: open/variants/versions/history/quit flow.

use super::*;
use crate::recipe_actions::{recipe_losses, recipe_name_error};


/// The persisted develop state belonging to the card that is on the canvas.
///
/// `recipe.json` / `pixels.json` describe whichever card was active at the
/// last save; a different persisted card gets its baseline from the matching
/// `variants.json.others[]` entry. Keeping this pair behind one owner prevents
/// a card switch from being mistaken for an edit by one of the four unsaved
/// consumers.
pub(crate) struct ActiveBaseline<'a> {
    pub(crate) recipe: &'a EditRecipe,
    pub(crate) origin: Option<&'a std::path::Path>,
}

/// The persisted develop of one saved card: only the halves the baseline
/// guard compares. Identity matching happens in `logic::strip`.
pub(crate) struct PersistedCard<'a> {
    pub(crate) recipe: &'a EditRecipe,
    pub(crate) origin: Option<&'a std::path::Path>,
}

impl AutoShadeApp {


    /// Restore persisted prefs (last folder, view mode, export options) and
    /// re-open the library the user was browsing. Window geometry itself is
    /// restored by eframe's own persistence layer.
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // The REAL event-loop context replaces default()'s headless one:
        // workers wake the loop through this clone (see `spawn_worker`).
        let mut app = Self {
            services: Services { egui_ctx: cc.egui_ctx.clone(), ..Services::default() },
            ..Self::default()
        };
        if let Some(prefs) =
            cc.storage.and_then(|s| eframe::get_value::<Prefs>(s, eframe::APP_KEY))
        {
            app.restore_prefs(prefs);
        }
        // The launch check of the interpreter the AI sidecars would run: a fact
        // about this machine, measured once before the first frame. It spawns
        // the interpreter only when the managed environment is not the answer,
        // so a prepared install costs one file check. After the prefs restore
        // (which decides the language) and outside it, so headless tests that
        // restore prefs run no interpreter.
        if let Some(note) = app.commands().refresh_python_check() {
            app.workspace.chrome_mut().startup_note = Some(note);
        }
        if !app.workspace.busy() {
            app.workspace.show_startup_note();
        }
        // The quit guard's two process-wide hooks (see `quit.rs`): the repaint
        // handle a vetoed quit wakes the UI with, and — on macOS — the
        // delegate method that does the vetoing. Both need a live event loop,
        // and this creation closure is the first moment there is one.
        crate::quit::set_ctx(&cc.egui_ctx);
        #[cfg(target_os = "macos")]
        if let Err(why) = crate::macos::install_quit_guard() {
            // Not fatal, and not silent: without that method ⌘Q behaves the
            // way it did before this batch, and the user is owed the fact.
            eprintln!(
                "warning: ⌘Q and Dock Quit will NOT ask about unsaved work — {why}. \
                 Close the window with its red button instead, which does ask."
            );
        }
        app
    }

    /// Restore the persisted settings through the same path at startup and
    /// in headless tests; no access to the user's actual prefs is needed.
    pub(crate) fn restore_prefs(&mut self, prefs: Prefs) {
        let app = self;
        app.aip.guide = prefs.guide;
        app.aip.style_strength = crate::logic::prefs::clamp01(prefs.style_strength);
        app.aip.grade_strength = crate::logic::prefs::clamp01(prefs.grade_strength);
        app.aip.fit_strength = crate::logic::prefs::clamp01(prefs.fit_strength);
        app.aip.send_style_ref_image = prefs.send_style_ref_image;
        app.aip.deep_think = prefs.deep_think;
        app.aip.style_embed = prefs.style_embed;
        app.aip.style_describe = prefs.style_describe;
        // Only a folder that still EXISTS is prefilled: a picker opened at
        // a deleted path lands wherever the OS decides (the same rule the
        // gallery restore above follows).
        app.aip.style_src_dir = prefs.style_src_dir.clone().filter(|d| d.is_dir());
        app.aip.style_raw_dir = prefs.style_raw_dir.clone().filter(|d| d.is_dir());
        app.aip.direction_adherence = autoshade::recipe::DirectionAdherence::new(prefs.direction_adherence).get();
        app.workspace.exp_mut().format = ExportFormat::from_pref(prefs.exp_format, prefs.save_jpeg);
        app.workspace.exp_mut().dest = ExportDest::from_pref(prefs.exp_dest);
        app.workspace.exp_mut().last_dir = prefs.last_export_dir.clone();
        app.workspace.exp_mut().save_denoise = prefs.save_denoise;
        app.workspace.exp_mut().save_denoise_strength = crate::logic::prefs::clamp01(prefs.save_denoise_strength);
        app.workspace.exp_mut().denoise_strength = crate::logic::prefs::clamp01(prefs.denoise_strength);
        app.workspace.exp_mut().baked_denoise_strength = crate::logic::prefs::clamp01(prefs.baked_denoise_strength);
        app.workspace.exp_mut().baked_save_denoise_strength = crate::logic::prefs::clamp01(prefs.baked_save_denoise_strength);
        app.workspace.fit_mut().zoned_fit = prefs.zoned_fit;
        app.workspace.fit_mut().zoned_four_regions = prefs.zoned_four_regions;
        app.workspace.fit_mut().fit_ai_judge = prefs.fit_ai_judge;
        app.workspace.fit_mut().fit_deep = prefs.fit_deep;
        app.workspace.fit_mut().reimagine_retry = prefs.reimagine_retry;
        app.workspace.fit_mut().ask_fit_after_generate = prefs.ask_fit_after_generate;
        app.workspace.genai_mut().adjust_quality = crate::logic::prefs::choice(prefs.adjust_quality);
        app.workspace.view_mut().mode = prefs.view_mode;
        app.workspace.exp_mut().long_edge = prefs.exp_long_edge;
        app.workspace.exp_mut().sharpen = crate::logic::prefs::clamp_sharpen(prefs.exp_sharpen);
        app.workspace.exp_mut().quality = crate::logic::prefs::clamp_quality(prefs.exp_quality);
        app.workspace.chrome_mut().show_clipping = prefs.show_clipping;
        // Restore the UI language (an older save without this key decoded to
        // `Lang::En` via `#[serde(default)]`, so this is always valid).
        app.lang = prefs.lang;
        // Restore the theme and re-apply it — main() installed the Dark
        // default before prefs were readable (same shape as the greeting).
        app.workspace.ui_mut().theme = prefs.theme;
        app.workspace.ui_mut().accent = prefs.accent;
        install_look(&app.services.egui_ctx, app.workspace.ui().theme, app.workspace.ui().accent);
        // The greeting was set by default() BEFORE the language was known —
        // re-issue it, or a Chinese install opens with one English line.
        app.workspace.set_status(tr(app.lang, K_ACTIONS_OPEN_A_PHOTO_OR_OPEN_A_FOLDER_TO_BROWSE_YOUR_LIB).into());
        // Each saved file advances to the current dial law; a current/future
        // file keeps the choices made under its own law. The wording of the
        // reset note uses the values the file actually carried.
        for era in prefs.prefs_era..PREFS_ERA {
            let migrated = crate::logic::prefs::migrate_era(era, prefs.denoise_strength, prefs.save_denoise_strength);
            app.workspace.exp_mut().denoise_strength = migrated.raw_detail;
            app.workspace.exp_mut().save_denoise_strength = migrated.raw_export;
            if let Some(v) = migrated.baked_detail {
                app.workspace.exp_mut().baked_denoise_strength = v;
            }
            if let Some(v) = migrated.baked_export {
                app.workspace.exp_mut().baked_save_denoise_strength = v;
            }
            if let Some((a, b)) = migrated.reset_note {
                let default = autoshade::denoise::DEFAULT_STRENGTH_RAW;
                app.workspace.chrome_mut().startup_note = Some(trf(
                    app.lang,
                    K_ACTIONS_AI_DENOISE_STRENGTH_FOR_RAW_FILES_WAS_RESET_TO_W,
                    &[
                        ("new", &format!("{:.0}", default * 100.0)),
                        ("a", &format!("{:.0}", a * 100.0)),
                        ("b", &format!("{:.0}", b * 100.0)),
                    ],
                ));
            }
        }
        // Only known color spaces and working-preview steps: a corrupt pref
        // must not produce an unrenderable space or a 1-px / 100-MP preview.
        app.workspace.exp_mut().space = crate::logic::prefs::known_space(prefs.exp_space);
        app.workspace.genai_mut().preview_edge = crate::logic::prefs::known_preview_edge(prefs.preview_edge);
        if let Some(dir) = prefs.gallery_dir.filter(|d| d.is_dir()) {
            app.commands().open_folder(dir);
        }
        // A restored folder's asynchronous status must not erase the note
        // before the user sees it. Its completion uses the same status line.
        if !app.workspace.busy() {
            app.workspace.show_startup_note();
        }
    }













    /// Match a live card to a persisted card. Stable IDs are authoritative
    /// when both sides carry one; an id-less legacy side falls back to kind +
    /// strip position, mirroring [`version_is_from`]'s ID-first rule.
    pub(crate) fn persisted_identity_matches(
        live: &Variant,
        live_position: usize,
        saved_kind: VariantKind,
        saved_position: usize,
        saved_id: Option<&str>,
    ) -> bool {
        crate::logic::strip::identity_matches(
            &live.id,
            live.kind,
            live_position,
            saved_kind,
            saved_position,
            saved_id,
        )
    }























}

impl Commands<'_> {

    /// Add a recipe file the user picked (our own recipe.json, or a Lightroom
    /// .xmp sidecar) to the library.
    pub(crate) fn import_recipe(&mut self) {
        // The panel only emits the verb; the dialog and the library writes are
        // the app's side effects.
        let Some(path) = rfd::FileDialog::new()
            .add_filter(tr(self.lang, K_ACTIONS_RECIPE_FILE), &["json", "xmp"])
            .pick_file()
        else {
            return;
        };
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        let name = match autoshade::recipe_library::check_name(&stem) {
            Ok(name) => name,
            Err(e) => {
                let text = recipe_name_error(self.lang, e);
                self.workspace.recipe_status(false, text);
                return;
            }
        };
        if autoshade::recipe_library::exists(&name) {
            let text = trf(self.lang, K_ACTIONS_A_RECIPE_NAMED_ALREADY_EXISTS, &[("name", &name)]);
            self.workspace.recipe_status(false, text);
            return;
        }
        let recipe = match autoshade::recipe_library::read_file(&path) {
            Ok(r) => r,
            Err(e) => {
                let text = trf(self.lang, K_ACTIONS_IS_NOT_A_RECIPE, &[("file", &path.display().to_string()), ("err", &e.to_string())]);
                self.workspace.recipe_status(false, text);
                return;
            }
        };
        let (entry, losses) = autoshade::recipe_library::portable(&recipe, false);
        match autoshade::recipe_library::create(&name, &entry) {
            Ok(()) => {
                self.workspace.refresh_recipes();
                self.workspace.rlib_mut().recipe_sel = Some(name.clone());
                let base = trf(self.lang, K_ACTIONS_IMPORTED, &[("name", &name)]);
                let text = if losses.is_empty() {
                    base
                } else {
                    format!(
                        "{} — {} {}",
                        base,
                        tr(self.lang, K_ACTIONS_LEFT_OUT_WHAT_BELONGS_TO_THE_SOURCE_PHOTO),
                        recipe_losses(self.lang, losses)
                    )
                };
                self.workspace.recipe_status(true, text);
            }
            Err(e) => {
                let text = trf(self.lang, K_ACTIONS_RECIPE_COULD_NOT_BE_SAVED, &[("name", &name), ("err", &e.to_string())]);
                self.workspace.recipe_status(false, text);
            }
        }
    }


    /// 「Save as jpg」: the single export of the card on screen (the ◈ card
    /// when the noise switch is on), as a jpg at the quality step, to the
    /// export destination (asked when the destination is "ask"). A name the
    /// app made is numbered (`-2`, `-3`, …) when a jpg of that name is
    /// already there; a file picked in the save window is replaced, as that
    /// window confirmed.
    pub(crate) fn start_normal_save(&mut self) {
        if self.editor.active_source_path().is_none() || self.workspace.busy() {
            return;
        }
        let name = self.editor.export_file_name_as("jpg");
        let (out, how) = match self.workspace.export_dest_dir() {
            Some(dir) => (dir.join(name), OutName::Numbered),
            None => {
                let Some(p) = self.workspace.pick_save_file(name) else { return };
                (normalize_export_target(p, "jpg"), OutName::Chosen)
            }
        };
        let export = self.workspace.normal_export_opts();
        // The folder picked in the save window is remembered when the render
        // starts — the workspace Export's rule (`export_to_chosen_path`), so
        // 「the folder you last saved to」 has somewhere to point.
        let picked = (how == OutName::Chosen).then(|| out.parent().map(|d| d.to_path_buf())).flatten();
        if self.start_render_with(out, export, None, false, how) {
            self.workspace.normal_mut().delivering = true;
            if picked.is_some() {
                self.workspace.exp_mut().last_dir = picked;
            }
        }
    }


    /// 「Apply this look to every photo in the folder」: the open photo's look
    /// pasted onto every photo of the library folder (crop left out), then —
    /// once the paste has landed whole (`after_apply_all_paste`) — every one
    /// rendered as a jpg. The library panel's own two verbs, as one. The
    /// jpgs' folder is settled FIRST: a folder dialog cancelled after the
    /// paste would have left every photo's look changed and nothing saved.
    pub(crate) fn start_apply_to_folder(&mut self) {
        if self.workspace.busy() || self.workspace.gal().gallery.is_empty() || self.editor.photo().src_path.is_none() {
            return;
        }
        let Some(dest) = self.workspace.apply_all_dest() else {
            // Cancelled: nothing was changed, and the line says nothing.
            self.workspace.status_mut().clear();
            return;
        };
        self.commit_pending_names();
        let targets = self.workspace.gal().gallery.clone();
        self.workspace.normal_mut().apply_all = Some(dest);
        self.start_paste_to(self.editor.doc().recipe.clone(), self.editor.photo().src_path.clone(), targets, false);
        if !self.workspace.busy() {
            self.workspace.normal_mut().apply_all = None;
        }
    }



    /// Duplicate a mask: its tuned sliders come along, its rasters are DETACHED
    /// copies (the version-load rule), so the twins never share a mutable file.
    pub(crate) fn duplicate_mask(&mut self, lang: Lang, i: usize) {
        if self.editor.doc().recipe.masks.len() >= 64 {
            let t = tr(lang, K_ACTIONS_MASK_LIMIT_REACHED_64_DELETE_ONE_FIRST);
            self.workspace.toast(ToastKind::Error, t.to_string());
            return;
        }
        let Some(mut clone) = self.editor.doc().recipe.masks.get(i).cloned() else { return };
        if let Some(p) = self.editor.photo().src_path.as_deref() {
            let mut tmp = EditRecipe { masks: vec![clone], ..Default::default() };
            autoshade::store::detach_rasters(p, &mut tmp, "mask-dup");
            clone = tmp.masks.remove(0);
        }
        if !clone.name.is_empty() {
            clone.name = format!("{} 2", clone.name);
        }
        crate::logic::command::apply(
            &mut self.editor.doc_mut().recipe,
            crate::logic::command::Command::InsertMask { after: i, mask: Box::new(clone) },
        );
        self.workspace.remap_mask_indices(|s| Some(if s > i { s + 1 } else { s }));
        self.workspace.masks_mut().sel_mask = Some(i + 1);
        self.workspace.masks_mut().sel_component = None;
        self.workspace.overlay_mut().overlay_stale = true;
        // The remap maps an armed Redraw(i)/brush(i) to ITSELF (s > i is false
        // at s == i) while the selection jumps to the copy — armed on the
        // original, pointed at the duplicate. Selection moved ⇒ same rule.
        if self.workspace.disarm_selection_bound_tools(self.workspace.masks().sel_mask) {
            let t = tr(
                self.lang,
                K_ACTIONS_THE_RASTER_EDIT_BRUSH_SESSION_ENDED_YOU_SELECTED,
            );
            self.workspace.toast(ToastKind::Error, t.to_string());
        }
        self.editor.session_mut().dirty = true;
    }

}

/// L12#3: disclose gallery names whose script no installed font can
/// draw — once per script per session. Runs over the CURRENT gallery
/// (folder-open granularity); pure classification, no file I/O.
pub(crate) fn disclose_undrawable_names(editor: &mut Editor, workspace: &mut WorkspaceView, lang: Lang) {
    disclose_undrawable_names_given(editor, workspace, lang, installed_scripts());
}

/// [`Self::disclose_undrawable_names`] against an explicit set of
/// installed scripts. The session's set is process-wide (filled once by
/// `install_fonts`), so a test that reads it sees whatever another test
/// in the same process installed; tests pass their own.
pub(crate) fn disclose_undrawable_names_given(
    editor: &mut Editor,
    workspace: &mut WorkspaceView,
    lang: Lang,
    installed: &[&'static str],
) {
    let stems: Vec<String> = workspace.gal().gallery
        .iter()
        .map(|p| autoshade::pipeline::stem(p).to_string())
        .collect();
    for stem in &stems {
        for (script, sample) in undrawable_scripts(stem, installed) {
            if editor.analysis_mut().disclosed_scripts.insert(script) {
                let t = trf(
                    lang,
                    K_ACTIONS_SOME_FILE_NAMES_USE_CHARACTERS_NO_INSTALLED_FONT,
                    &[("sample", &sample.to_string())],
                );
                workspace.toast(ToastKind::Error, t.clone());
                workspace.set_status(t);
            }
        }
    }
}
