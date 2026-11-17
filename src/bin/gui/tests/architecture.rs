// One part of the GUI's tests (src/bin/gui/tests.rs includes it): the
// architecture seam between the view, the headless logic and the runtime.
//
// These are SOURCE tests on purpose: the drift they guard against is a line
// that goes missing or an import that creeps back, and neither has a runtime
// signature until the next feature pays for it. The pair pins the deal made in
// `logic/mod.rs` and the panel projections:
//
//  * logic is headless — no widget code, no rendered text, no worker spawn,
//    no dialog, no message enum;
//  * a migrated panel does not mutate the recipe, the app's own state or the
//    filesystem: it draws and emits `logic::Action`s.

    fn collect_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("the source tree is listable") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                collect_rs(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// The logic layer is headless: it may read and write data, but reaching
    /// for the view or the runtime is what turned one policy change into
    /// hundreds of edits.
    ///
    /// MUTATION: call `egui::Context` (or a worker, or `tr`) from a file under
    /// `logic/` and this names the file and the needle.
    #[test]
    fn the_logic_layer_is_headless() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("bin")
            .join("gui")
            .join("logic");
        let mut files = Vec::new();
        collect_rs(&dir, &mut files);
        assert!(!files.is_empty(), "no logic sources found under {}", dir.display());
        for file in files {
            let src = std::fs::read_to_string(&file).expect("logic source readable");
            // Spellings the Rust-blind scanners (this one's own source included)
            // would otherwise read as real call sites are built at compile time:
            // a quoted call spelling reads as a call to the i18n audit.
            let calls = [concat!("tr", "("), concat!("trf", "("), concat!("spawn_worker", "(")];
            let plain = ["egui::", "AutoShadeApp", "rfd::", "Msg::", "ToastKind", "std::thread"];
            for needle in calls.iter().chain(plain.iter()) {
                assert!(
                    !src.contains(needle),
                    "{} reaches into the view/runtime with `{needle}` — logic returns an Effect instead",
                    file.display()
                );
            }
        }
    }

    /// The migrated panels are view-only: they read state, draw, and return
    /// actions. A mutation, a worker, a dialog or a store write here would
    /// re-fork the business logic these migrations removed.
    ///
    /// MUTATION: put `.as_mut()` or `rfd::` back in a listed panel and this
    /// fails with the file and the needle.
    #[test]
    fn the_migrated_panels_stay_behind_the_seam() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("bin")
            .join("gui")
            .join("panels");
        let common = [
            concat!("spawn_worker", "("),
            "rfd::",
            "std::fs::",
            "autoshade::store::",
            "Msg::",
            "ToastKind",
            ".as_mut()",
            "self.workspace.status() =",
            "self.workspace.toast(",
            "self.workspace.busy() =",
            "self.editor.session().dirty =",
        ];
        // The lens-blur section draws through the panel projection now. Its
        // draw must not start the depth worker, open a dialog or reach into
        // the recipe.
        let gui = root.parent().expect("the gui directory");
        let panel_view = std::fs::read_to_string(gui.join("panel_view.rs")).expect("the projection source is readable");
        let lens = &panel_view[panel_view.find("fn dev_lens_blur(").expect("the lens-blur section")..];
        let lens = &lens[..lens.find("\n    pub(crate) fn ").expect("the next projection method")];
        for needle in common.into_iter().chain(["on_depth_estimated", "start_depth"]) {
            assert!(
                !lens.contains(needle),
                "the lens-blur section reaches past the seam with `{needle}` — emit a logic::Action instead"
            );
        }
        // `gallery.rs` — the folder dialog, the thumbnail worker, the selection
        // and the texture window are the runtime/logic's now.
        let gallery = std::fs::read_to_string(root.join("gallery.rs")).expect("the panel source is readable");
        for needle in common.into_iter().chain([
            "open_gallery_index",
            "request_thumb(",
            "self.workspace.gal().thumbs.retain",
            "self.workspace.gal().thumb_requested",
            "multi_sel.insert",
            "multi_sel.remove",
            "multi_sel.clear",
        ]) {
            assert!(
                !gallery.contains(needle),
                "panels/gallery.rs reaches past the seam with `{needle}` — emit a logic::Action instead"
            );
        // `settings.rs` — its verbs (save / fetch models / install the Python
        // environment) are actions now: no worker, no dialog, no locked write
        // and no call to the moved business methods from the draw code.
        let settings = std::fs::read_to_string(root.join("settings.rs")).expect("the panel source is readable");
        for needle in [
            concat!("spawn_worker", "("),
            "rfd::",
            "Msg::",
            "update_local_settings",
            "save_settings_form(",
            "fetch_models(",
            "start_python_env_install(",
        ] {
            assert!(
                !settings.contains(needle),
                "panels/settings.rs reaches past the seam with `{needle}` — emit a logic::Action instead"
            );
        }
        // `retouch.rs` — its worker starts (fill / adjust / heal / denoise /
        // stack / clone / reimagine / style prompt) live in
        // `retouch_actions.rs` now: the panel draws, samples the brush and
        // emits verbs.
        let retouch = std::fs::read_to_string(root.join("retouch.rs")).expect("the panel source is readable");
        for needle in [
            concat!("spawn_worker", "("),
            "Msg::",
            "ToastKind",
            concat!("crate::budget::heavy_permit", "("),
            "autoshade::store::",
            "rfd::",
            "self.editor.doc().recipe.pixel_layers.push(",
        ] {
            assert!(
                !retouch.contains(needle),
                "panels/retouch.rs reaches past the seam with `{needle}` — emit a logic::Action instead"
            );
        }
        // `ai.rs` — the AI section's starts (library status/build, reimagine,
        // generated-card adjust, reverse-fit, style prompt) are actions now:
        // the panel draws and emits verbs.
        let ai = std::fs::read_to_string(root.join("ai.rs")).expect("the panel source is readable");
        for needle in [
            concat!("spawn_worker", "("),
            "Msg::",
            "ToastKind",
            concat!("crate::budget::heavy_permit", "("),
            "autoshade::store::",
            "rfd::",
            "start_style_info(",
            "start_style_build(",
            "start_reimagine(",
            "start_adjust(",
            "start_fit(",
            "start_style_prompt(",
        ] {
            assert!(
                !ai.contains(needle),
                "panels/ai.rs reaches past the seam with `{needle}` — emit a logic::Action instead"
            );
        }
        // `develop.rs` — its mask-list and version-list verbs are actions now
        // (`DevelopAction`): moving/duplicating a mask and loading/deleting a
        // version are the app layer's, and the panel no longer starts a worker
        // or touches the store from draw code.
        let develop = std::fs::read_to_string(root.join("develop.rs")).expect("the panel source is readable");
        for needle in [
            concat!("spawn_worker", "("),
            "Msg::",
            "rfd::",
            concat!("crate::budget::heavy_permit", "("),
            "autoshade::store::detach_rasters(",
            "autoshade::store::with_develop_lock(",
            "autoshade::store::delete_version(",
            "self.start_",
        ] {
            assert!(
                !develop.contains(needle),
                "panels/develop.rs reaches past the seam with `{needle}` — emit a logic::Action instead"
            );
        }
        // `recipes.rs` — the library's business (list/save/apply/rename/delete)
        // lives in `recipe_actions.rs`; the window draws and emits verbs. The
        // preview/details panes still READ an entry to draw it — a read is not
        // a mutation — but nothing here writes the library.
        let recipes = std::fs::read_to_string(root.join("recipes.rs")).expect("the panel source is readable");
        for needle in [
            concat!("spawn_worker", "("),
            "Msg::",
            "rfd::",
            concat!("crate::budget::heavy_permit", "("),
            "recipe_library::create(",
            "recipe_library::remove(",
            "save_current_recipe(",
            "apply_recipe_to_open(",
            "apply_recipe_to_selection(",
            "update_recipe_from_open(",
            "rename_recipe(",
            "delete_recipe(",
            "ToastKind",
        ] {
            assert!(
                !recipes.contains(needle),
                "panels/recipes.rs reaches past the seam with `{needle}` — emit a logic::Action instead"
            );
        }
        // `portrait.rs` — finding the people, the skin switch's bookkeeping,
        // blemish removal and the depth landing live in `portrait_actions.rs`;
        // the panel draws and emits PortraitActions.
        let portrait = std::fs::read_to_string(root.join("portrait.rs")).expect("the panel source is readable");
        for needle in [
            concat!("spawn_worker", "("),
            "Msg::",
            "ToastKind",
            concat!("crate::budget::heavy_permit", "("),
            "autoshade::store::",
            "start_find_people(",
            "start_remove_blemishes(",
            "set_keep_person_sharp(",
            "set_keep_skin(",
            "on_portrait_found(",
            "after_portrait_depth(",
            "guard_portrait_skin(",
        ] {
            assert!(
                !portrait.contains(needle),
                "panels/portrait.rs reaches past the seam with `{needle}` — emit a logic::Action instead"
            );
        }
        // `normal.rs` — the Save group's hand-off/landing words and the noise
        // switch's card dance live in `normal_actions.rs`; the panel draws and
        // emits NormalActions.
        let normal = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("bin")
                .join("gui")
                .join("panels")
                .join("normal.rs"),
        )
        .expect("the panel source is readable");
        for needle in [
            concat!("spawn_worker", "("),
            "Msg::",
            "ToastKind",
            concat!("crate::budget::heavy_permit", "("),
            "autoshade::store::",
            "start_batch_render_for(",
            "start_render_with(",
            "start_ai_denoise(",
            "set_normal_denoise(",
            "take_look_history(",
            "carry_look_history(",
            "normal_delivery_words(",
            "after_apply_all_paste(",
        ] {
            assert!(
                !normal.contains(needle),
                "panels/normal.rs reaches past the seam with `{needle}` — emit a logic::Action instead"
            );
        }
        // `canvas.rs` — the view's biggest file. Its gestures emit actions
        // (crop / pick / mask / region); it starts no worker, opens no dialog,
        // takes no permit, writes no store and does not replace the recipe.
        let canvas = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("bin")
                .join("gui")
                .join("canvas.rs"),
        )
        .expect("the canvas source is readable");
        for needle in [
            concat!("spawn_worker", "("),
            "Msg::",
            "rfd::",
            concat!("crate::budget::heavy_permit", "("),
            "autoshade::store::",
            "self.start_",
            "self.editor.doc().recipe =",
            "self.editor.doc().recipe.masks.push(",
            "self.editor.doc().recipe.point_colors.push(",
            "self.editor.doc().recipe.pixel_layers.push(",
        ] {
            assert!(
                !canvas.contains(needle),
                "canvas.rs reaches past the seam with `{needle}` — emit a logic::Action instead"
            );
        }
        // The two pure-widget panels: no business at all.
        for file in ["grade_wheel.rs", "histogram.rs"] {
            let src = std::fs::read_to_string(root.join(file)).expect("the panel source is readable");
            for needle in [
                concat!("spawn_worker", "("),
                "Msg::",
                "rfd::",
                concat!("crate::budget::heavy_permit", "("),
                "autoshade::store::",
                "ToastKind",
                "self.start_",
                "self.editor.doc().recipe =",
                "self.editor.doc().recipe.masks.push(",
            ] {
                assert!(
                    !src.contains(needle),
                    "panels/{file} reaches past the seam with `{needle}` — emit a logic::Action instead"
                );
            }
        }
        // The frame (`app.rs`) opens no dialog: the empty canvas's open verbs
        // are GalleryActions now.
        let app = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("bin")
                .join("gui")
                .join("app.rs"),
        )
        .expect("the app source is readable");
        for needle in ["rfd::", "open_photo_dialog("] {
            assert!(
                !app.contains(needle),
                "app.rs opens a dialog with `{needle}` — emit a logic::Action instead"
            );
        }
        }
    }
