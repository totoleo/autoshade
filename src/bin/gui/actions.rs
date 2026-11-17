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
