//! The recipe library's business: listing, saving, applying, renaming and
//! deleting library entries, and the words those outcomes say.
//!
//! The window (`panels/recipes.rs`) draws and emits verbs; the import dialog
//! lives in `actions.rs` beside this file.

use crate::*;

impl Commands<'_> {


    /// The name a blank field saves under: the open photo's stem.
    fn recipe_default_name(&self) -> String {
        match self.editor.photo().src_path.as_deref() {
            Some(p) => autoshade::pipeline::stem(p).to_string(),
            None => tr(self.lang, K_ACTIONS_RECIPE).to_string(),
        }
    }

    /// Localized wording for a refused name.

    /// What a portable entry left behind, as one list of user-facing fragments.


    /// Save the open photo's develop into the library under a name.
    pub(crate) fn save_current_recipe(&mut self) {
        let Some(src) = self.editor.photo().src_path.clone() else {
            return;
        };
        if self.workspace.busy() {
            return;
        }
        // Every other persistence-shaped verb commits open name boxes first.
        self.commit_pending_names();
        let typed = self.workspace.rlib().recipe_name_buf.trim();
        let name = if typed.is_empty() { self.recipe_default_name() } else { typed.to_string() };
        let name = match autoshade::recipe_library::check_name(&name) {
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
        let (entry, losses) = autoshade::recipe_library::portable(&self.editor.doc().recipe, false);
        match autoshade::recipe_library::create(&name, &entry) {
            Ok(()) => {
                let _ = src;
                self.workspace.rlib_mut().recipe_name_buf.clear();
                self.workspace.refresh_recipes();
                self.workspace.rlib_mut().recipe_sel = Some(name.clone());
                let base = trf(self.lang, K_ACTIONS_SAVED_RECIPE, &[("name", &name)]);
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

    /// Put a library entry on the canvas. Nothing is written until Ctrl+S —
    /// the same stance as loading a version.
    pub(crate) fn apply_recipe_to_open(&mut self, name: &str) {
        if self.workspace.busy() {
            return;
        }
        let Some(src) = self.editor.photo().src_path.clone() else {
            return;
        };
        self.commit_pending_names();
        let entry = match autoshade::recipe_library::read(name) {
            Ok(r) => r,
            Err(e) => {
                let text = trf(self.lang, K_ACTIONS_RECIPE_COULD_NOT_BE_READ, &[("name", name), ("err", &e.to_string())]);
                self.workspace.recipe_status(false, text);
                return;
            }
        };
        // The target's own calibration and baked-source rules, through the
        // same shaper the clipboard paste uses.
        let mut live = crate::persist::paste_recipe_for(&src, &entry);
        if self.editor.skin_held_for(&self.workspace, &src) {
            autoshade::face::keep_skin_natural_in(&mut live);
        }
        if self.editor.active_on_ai_pixels() {
            live.strip_baked_start();
            live.as_shot_k = None;
            live.as_shot_tint = None;
        }
        let dropped = live.clamp();
        if !dropped.is_empty() {
            let t = trf(
                self.lang,
                K_CANVAS_RECIPE_LIMITS_DISCARDED_MASK_S_COMPONENT_S_CURVE,
                &[
                    ("n", &dropped.dropped_masks.to_string()),
                    ("m", &dropped.dropped_components.to_string()),
                    ("c", &dropped.truncated_curve_points.to_string()),
                    ("s", &dropped.truncated_string_bytes.to_string()),
                ],
            );
            self.workspace.toast(ToastKind::Error, t);
        }
        if let Err(e) = autoshade::pipeline::migrate_recipe_coord_frame(&src, &mut live) {
            self.workspace.toast(
                ToastKind::Error,
                coord_migration_failure_sentence(self.lang, &e.to_string()),
            );
        }
        self.editor.doc_mut().recipe = live;
        self.editor.session_mut().dirty = true;
        // A recipe placed on the pristine ✨ card is an edit like any other:
        // it lands on a new ✎ card (the immutability rule).
        self.fork_edited_card();
        self.editor.resync_recipe_display(&mut self.workspace);
        let text = trf(self.lang, K_ACTIONS_APPLIED_RECIPE_CTRL_Z_RETURNS_TO_BEFORE, &[("name", name)]);
        self.workspace.recipe_status(true, text);
    }

    /// Write a library entry into the selected photos' develops (the paste
    /// path: recipe.json per photo, XMP for RAWs, nothing rendered).
    pub(crate) fn apply_recipe_to_selection(&mut self, name: &str) {
        if self.workspace.busy() {
            return;
        }
        let targets: Vec<PathBuf> = {
            let mut idx: Vec<usize> = self.workspace.gal().multi_sel.iter().copied().collect();
            idx.sort_unstable();
            idx.into_iter().filter_map(|i| self.workspace.gal().gallery.get(i).cloned()).collect()
        };
        if targets.is_empty() {
            return;
        }
        let entry = match autoshade::recipe_library::read(name) {
            Ok(r) => r,
            Err(e) => {
                let text = trf(self.lang, K_ACTIONS_RECIPE_COULD_NOT_BE_READ, &[("name", name), ("err", &e.to_string())]);
                self.workspace.recipe_status(false, text);
                return;
            }
        };
        self.commit_pending_names();
        self.start_paste_to(entry, None, targets, false);
    }

    /// Replace a library entry's content with the open photo's current look.
    pub(crate) fn update_recipe_from_open(&mut self, name: &str) {
        if self.workspace.busy() || self.editor.photo().src_path.is_none() {
            return;
        }
        self.commit_pending_names();
        let (entry, losses) = autoshade::recipe_library::portable(&self.editor.doc().recipe, false);
        match autoshade::recipe_library::replace(name, &entry) {
            Ok(()) => {
                self.workspace.refresh_recipes();
                let base = trf(self.lang, K_ACTIONS_UPDATED_RECIPE, &[("name", name)]);
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
                let text = trf(self.lang, K_ACTIONS_RECIPE_COULD_NOT_BE_SAVED, &[("name", name), ("err", &e.to_string())]);
                self.workspace.recipe_status(false, text);
            }
        }
    }

    /// Rename an entry (`from` -> `to`; the file moves with it).
    pub(crate) fn rename_recipe(&mut self, from: &str, to: &str) {
        let to = match autoshade::recipe_library::check_name(to) {
            Ok(name) => name,
            Err(e) => {
                let text = recipe_name_error(self.lang, e);
                self.workspace.recipe_status(false, text);
                return;
            }
        };
        if to == from {
            return;
        }
        if autoshade::recipe_library::exists(&to) {
            let text = trf(self.lang, K_ACTIONS_A_RECIPE_NAMED_ALREADY_EXISTS, &[("name", &to)]);
            self.workspace.recipe_status(false, text);
            return;
        }
        match autoshade::recipe_library::rename(from, &to) {
            Ok(()) => {
                self.workspace.refresh_recipes();
                self.workspace.rlib_mut().recipe_sel = Some(to.clone());
                let text = trf(self.lang, K_ACTIONS_RENAMED_TO, &[("name", &to)]);
                self.workspace.recipe_status(true, text);
            }
            Err(e) => {
                let text = trf(self.lang, K_ACTIONS_RECIPE_COULD_NOT_BE_RENAMED, &[("name", from), ("err", &e.to_string())]);
                self.workspace.recipe_status(false, text);
            }
        }
    }

    /// Delete an entry's file. The photos it was applied to keep their own
    /// saved develops — this removes the library copy only.
    pub(crate) fn delete_recipe(&mut self, name: &str) {
        match autoshade::recipe_library::delete(name) {
            Ok(()) => {
                self.workspace.refresh_recipes();
                self.workspace.rlib_mut().recipe_sel = None;
                self.workspace.rlib_mut().recipe_delete_armed = None;
                let text = trf(self.lang, K_ACTIONS_DELETED_RECIPE, &[("name", name)]);
                self.workspace.recipe_status(true, text);
            }
            Err(e) => {
                let text = trf(self.lang, K_ACTIONS_RECIPE_COULD_NOT_BE_DELETED, &[("name", name), ("err", &e.to_string())]);
                self.workspace.recipe_status(false, text);
            }
        }
    }
}

pub(crate) fn recipe_name_error(lang: Lang, e: autoshade::recipe_library::NameError) -> String {
    use autoshade::recipe_library::NameError as N;
    match e {
        N::Empty => tr(lang, K_ACTIONS_A_RECIPE_NEEDS_A_NAME).to_string(),
        N::TooLong => trf(
            lang,
            K_ACTIONS_THE_NAME_IS_LONGER_THAN_CHARACTERS,
            &[("n", &autoshade::store::MAX_STORE_NAME.to_string())],
        ),
        N::Illegal(c) => trf(lang, K_ACTIONS_THE_NAME_CANNOT_CONTAIN, &[("char", &c.to_string())]),
    }
}

pub(crate) fn recipe_losses(lang: Lang, losses: autoshade::recipe_library::Losses) -> String {
    let lang = lang;
    let mut parts: Vec<String> = Vec::new();
    if losses.masks > 0 {
        parts.push(trf(lang, K_ACTIONS_BITMAP_MASK_S, &[("n", &losses.masks.to_string())]));
    }
    if losses.colour_field {
        parts.push(tr(lang, K_ACTIONS_THE_COLOUR_FIELD).to_string());
    }
    if losses.layers > 0 {
        parts.push(trf(lang, K_ACTIONS_FILL_LAYER_S, &[("n", &losses.layers.to_string())]));
    }
    if losses.lens_blur {
        parts.push(tr(lang, K_ACTIONS_THE_LENS_BLUR).to_string());
    }
    if losses.geometry {
        parts.push(tr(lang, K_ACTIONS_CROP_STRAIGHTEN).to_string());
    }
    parts.join(", ")
}
