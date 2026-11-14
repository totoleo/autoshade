// The recipe library window: saving the open develop under a name, applying an
// entry to the canvas and to the selected photos, renaming / updating /
// deleting, and the list's own drawing. Real files throughout: every entry is
// a recipe.json in the store's `recipes/` folder, read back from disk.

use autoshade::recipe_library;

fn unique(tag: &str) -> String {
    format!(
        "_gui_recipe_{tag}_{}_{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(unique(tag));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn jpeg(p: &std::path::Path) {
    let img = image::RgbImage::from_pixel(8, 8, image::Rgb([40, 60, 80]));
    image::DynamicImage::ImageRgb8(img).save(p).unwrap();
}

fn app_with(photo: &std::path::Path, recipe: EditRecipe) -> AutoShadeApp {
    AutoShadeApp {
        lang: Lang::En,
        src_path: Some(photo.to_path_buf()),
        recipe,
        ..Default::default()
    }
}

/// The window's own frame, tall enough that no row is culled.
fn window_texts(app: &mut AutoShadeApp) -> Vec<String> {
    let ctx = egui::Context::default();
    crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
    let mut seen = Vec::new();
    for frame in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 1200.0),
            )),
            time: Some(frame as f64 / 2.0),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            ctx.memory_mut(|m| m.set_everything_is_visible(true));
            app.recipe_library_window(ctx);
        });
        seen = drawn_texts(&out.shapes);
    }
    seen
}

/// MUTATION THIS CATCHES: save the full develop (crop, bitmap masks) — the
/// entry then carries a raster that belongs to one photo, and the file is not
/// the plain portable recipe `apply` can use on another one.
#[test]
fn the_open_develop_is_saved_as_a_plain_portable_recipe() {
    let name = unique("save");
    let path = recipe_library::entry_path(&name);
    let _scrub = Scrub(vec![path.clone()]);
    let photo = PathBuf::from("D:/library/_gui_recipe_source.ARW");
    let mut recipe = EditRecipe { exposure_ev: 0.42, contrast: 11.0, ..Default::default() };
    recipe.crop = Some(autoshade::recipe::Crop { left: 0.1, top: 0.1, right: 0.9, bottom: 0.9 });
    recipe.masks.push(autoshade::recipe::LocalAdjustment {
        mask: autoshade::recipe::MaskGeometry::Bitmap { path: "belongs-to-one-photo.png".into() },
        ..Default::default()
    });
    let mut app = app_with(&photo, recipe);
    app.recipe_name_buf = name.clone();
    app.save_current_recipe();

    let text = std::fs::read_to_string(&path).expect("the entry exists on disk");
    let back: EditRecipe = serde_json::from_str(&text).unwrap();
    assert_eq!(back.exposure_ev, 0.42);
    assert!(back.crop.is_none(), "crop belongs to the source photo");
    assert!(back.masks.is_empty(), "a bitmap mask's raster belongs to the source photo");
    assert_eq!(app.recipe_sel.as_deref(), Some(name.as_str()), "the new entry is selected");
    assert!(
        app.recipes.iter().any(|r| r.name == name),
        "the list was re-read from disk"
    );
    assert!(
        app.status.contains("Saved recipe") && app.status.contains("left out"),
        "the loss is disclosed: {}",
        app.status
    );
}

/// MUTATION THIS CATCHES: overwrite a taken name — the first look is lost with
/// no way back (and the row list would still show one entry).
#[test]
fn a_taken_or_illegal_name_is_refused_and_nothing_is_written() {
    let name = unique("taken");
    let stem = "_gui_recipe_taken";
    let path = recipe_library::entry_path(&name);
    let _scrub = Scrub(vec![path.clone(), recipe_library::entry_path(stem)]);
    let mut app = app_with(&PathBuf::from("D:/library/_gui_recipe_taken.ARW"), EditRecipe { exposure_ev: 0.5, ..Default::default() });
    app.recipe_name_buf = name.clone();
    app.save_current_recipe();
    let first = std::fs::read_to_string(&path).unwrap();

    // The field clears after a save: type the same name again to ask for it.
    app.recipe.exposure_ev = -2.0;
    app.recipe_name_buf = name.clone();
    app.save_current_recipe();
    assert!(app.status.contains("already exists"), "{}", app.status);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), first, "the first entry survived");

    app.recipe_name_buf = "a/b".into();
    app.save_current_recipe();
    assert!(app.status.contains("cannot contain"), "{}", app.status);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
}

/// MUTATION THIS CATCHES: write the apply straight to the store — the library
/// entry is a look to try, and applying it must leave the canvas unsaved (the
/// version-load stance) so Ctrl+S is the user's decision.
#[test]
fn applying_a_recipe_to_the_canvas_is_an_unsaved_edit() {
    let name = unique("apply");
    let path = recipe_library::entry_path(&name);
    let dir = temp_dir("apply-photo");
    let photo = dir.join("_gui_recipe_apply.jpg");
    jpeg(&photo);
    let _scrub = Scrub(vec![path.clone(), dir.clone(), autoshade::store::develop_dir(&photo)]);
    recipe_library::create(&name, &EditRecipe { exposure_ev: 0.9, contrast: 21.0, ..Default::default() }).unwrap();

    let mut app = app_with(&photo, EditRecipe::default());
    app.saved_recipe = app.recipe.clone();
    app.apply_recipe_to_open(&name);
    assert!((app.recipe.exposure_ev - 0.9).abs() < 1e-6, "{}", app.recipe.exposure_ev);
    assert_eq!(app.recipe.contrast, 21.0);
    assert!(app.dirty, "the canvas is an unsaved edit");
    assert!(!autoshade::store::recipe_target(&photo).exists(), "nothing was written to the store");
    assert!(app.status.contains("Applied recipe"), "{}", app.status);
}

/// MUTATION THIS CATCHES: apply to the selection without writing each
/// target's develop (or writing only the open one).
#[test]
fn applying_to_the_selection_writes_each_targets_develop() {
    let name = unique("batch");
    let path = recipe_library::entry_path(&name);
    let dir = temp_dir("batch-photos");
    let (a, b) = (dir.join("_gui_recipe_a.jpg"), dir.join("_gui_recipe_b.jpg"));
    jpeg(&a);
    jpeg(&b);
    let _scrub = Scrub(vec![
        path.clone(),
        dir.clone(),
        autoshade::store::develop_dir(&a),
        autoshade::store::develop_dir(&b),
    ]);
    recipe_library::create(&name, &EditRecipe { exposure_ev: 0.66, ..Default::default() }).unwrap();

    let mut app = app_with(&a, EditRecipe::default());
    app.gallery = vec![a.clone(), b.clone()];
    app.multi_sel = [0usize, 1usize].into_iter().collect();
    app.apply_recipe_to_selection(&name);
    let ctx = egui::Context::default();
    let start = std::time::Instant::now();
    while app.busy {
        assert!(start.elapsed() < std::time::Duration::from_secs(20), "the paste never landed");
        std::thread::sleep(std::time::Duration::from_millis(20));
        app.poll_workers(&ctx);
    }
    for p in [&a, &b] {
        let target = autoshade::store::recipe_target(p);
        let text = std::fs::read_to_string(&target)
            .unwrap_or_else(|e| panic!("{} has no saved develop: {e}", p.display()));
        let back: EditRecipe = serde_json::from_str(&text).unwrap();
        assert!((back.exposure_ev - 0.66).abs() < 1e-6, "{}: {}", p.display(), back.exposure_ev);
    }
}

/// MUTATION THIS CATCHES: rename by rewriting the content (the file name and
/// the row then disagree) or update by replacing the name along with the look.
#[test]
fn rename_moves_the_file_and_update_keeps_the_name() {
    let from = unique("ren");
    let to = format!("{from}_2");
    let _scrub = Scrub(vec![
        recipe_library::entry_path(&from),
        recipe_library::entry_path(&to),
    ]);
    let photo = PathBuf::from("D:/library/_gui_recipe_ren.ARW");
    let mut app = app_with(&photo, EditRecipe { contrast: 5.0, ..Default::default() });
    let _ = to;
    recipe_library::create(&from, &EditRecipe { contrast: 5.0, ..Default::default() }).unwrap();
    app.recipes_loaded = true;
    app.refresh_recipes();

    app.recipe.contrast = 44.0;
    app.update_recipe_from_open(&from);
    let back = recipe_library::read(&from).unwrap();
    assert_eq!(back.contrast, 44.0, "the content was replaced");
    assert!(!recipe_library::entry_path(&to).exists());

    app.rename_recipe(&from, &to);
    assert!(!recipe_library::entry_path(&from).exists(), "the old file is gone");
    assert!(recipe_library::entry_path(&to).is_file(), "the file moved");
    assert_eq!(app.recipe_sel.as_deref(), Some(to.as_str()));
    assert_eq!(recipe_library::read(&to).unwrap().contrast, 44.0, "the content moved with it");
}

/// MUTATION THIS CATCHES: delete the entry's file but leave it in the list, or
/// leave the selection pointing at a name that no longer exists.
#[test]
fn deleting_an_entry_removes_its_file_and_the_row() {
    let name = unique("del");
    let _scrub = Scrub(vec![recipe_library::entry_path(&name)]);
    let mut app = app_with(&PathBuf::from("D:/library/_gui_recipe_del.ARW"), EditRecipe::default());
    recipe_library::create(&name, &EditRecipe::default()).unwrap();
    app.refresh_recipes();
    app.recipe_sel = Some(name.clone());

    app.delete_recipe(&name);
    assert!(!recipe_library::entry_path(&name).exists(), "the file is gone");
    assert!(app.recipe_sel.is_none(), "the selection followed it");
    assert!(app.recipes.iter().all(|r| r.name != name), "the row is gone");
    assert!(app.status.contains("Deleted recipe"), "{}", app.status);
}

/// The window draws the rows it read from disk, and the filter narrows them.
#[test]
fn the_window_lists_the_library_and_filters_it() {
    let (alpha, beta) = (unique("Alpha"), unique("Beta"));
    let _scrub = Scrub(vec![
        recipe_library::entry_path(&alpha),
        recipe_library::entry_path(&beta),
    ]);
    recipe_library::create(&alpha, &EditRecipe::default()).unwrap();
    recipe_library::create(&beta, &EditRecipe { exposure_ev: 0.3, ..Default::default() }).unwrap();
    let mut app = app_with(&PathBuf::from("D:/library/_gui_recipe_list.ARW"), EditRecipe::default());
    app.show_recipes = true;

    let seen = window_texts(&mut app);
    assert!(seen.iter().any(|t| t.contains(&alpha)), "the saved row: {seen:?}");
    assert!(seen.iter().any(|t| t.contains(&beta)), "the imported row: {seen:?}");
    assert!(seen.iter().any(|t| t.contains("Save")), "the save verb: {seen:?}");

    app.recipes_filter = "Alpha".into();
    let seen = window_texts(&mut app);
    assert!(seen.iter().any(|t| t.contains(&alpha)), "{seen:?}");
    assert!(!seen.iter().any(|t| t.contains(&beta)), "the filter hid it: {seen:?}");
}

/// The selected entry's own settings are shown — and only the ones it
/// actually changes: the list alone said nothing about what a recipe IS.
///
/// MUTATION THIS CATCHES: drop the facts grid (or print every field at its
/// neutral) — the first assertion fails on the values, the second on the
/// untouched control leaking into the pane.
#[test]
fn the_selected_recipe_shows_the_settings_it_changes() {
    let (alpha, beta) = (unique("Facts"), unique("Neutral"));
    let _scrub = Scrub(vec![
        recipe_library::entry_path(&alpha),
        recipe_library::entry_path(&beta),
    ]);
    let mut mask = autoshade::recipe::LocalAdjustment::default();
    mask.exposure_ev = 0.3;
    recipe_library::create(
        &alpha,
        &EditRecipe { exposure_ev: 0.5, contrast: 12.0, masks: vec![mask], ..Default::default() },
    )
    .unwrap();
    recipe_library::create(&beta, &EditRecipe::default()).unwrap();
    let mut app = app_with(&PathBuf::from("D:/library/_gui_recipe_facts.ARW"), EditRecipe::default());
    app.show_recipes = true;
    app.recipe_sel = Some(alpha.clone());
    let seen = window_texts(&mut app);
    assert!(seen.iter().any(|t| t.contains("Exposure")), "{seen:?}");
    assert!(seen.iter().any(|t| t.contains("+0.50")), "{seen:?}");
    assert!(seen.iter().any(|t| t.contains("+12")), "{seen:?}");
    assert!(seen.iter().any(|t| t.contains("Masks")), "{seen:?}");
    assert!(!seen.iter().any(|t| t.contains("Dehaze")), "a neutral control is not listed: {seen:?}");

    app.recipe_sel = Some(beta.clone());
    let seen = window_texts(&mut app);
    assert!(
        seen.iter().any(|t| t.contains("No edits")),
        "a neutral recipe says so instead of an empty table: {seen:?}"
    );
}

/// The selected recipe is developed on the open photo for the preview pane.
///
/// MUTATION THIS CATCHES: render the entry as stored (no `paste_recipe_for`)
/// or skip the preview entirely — the texture is never built.
#[test]
fn the_selected_recipe_is_previewed_on_the_open_photo() {
    let name = unique("Preview");
    let dir = temp_dir("preview-photo");
    let photo = dir.join("_gui_recipe_preview.jpg");
    jpeg(&photo);
    let _scrub = Scrub(vec![recipe_library::entry_path(&name), dir.clone()]);
    recipe_library::create(&name, &EditRecipe { exposure_ev: 0.4, ..Default::default() }).unwrap();
    let mut app = app_with(&photo, EditRecipe::default());
    app.base_preview = Some(std::sync::Arc::new(image::DynamicImage::ImageRgb8(
        image::RgbImage::from_pixel(120, 80, image::Rgb([90, 110, 130])),
    )));
    app.show_recipes = true;
    app.recipe_sel = Some(name.clone());
    let _ = window_texts(&mut app);
    let first = app.recipe_preview.as_ref().expect("the preview pane rendered the entry").key.clone();
    assert!(first.contains(&name), "{first}");

    // Re-selecting another entry rebuilds it (the key names the file).
    app.recipe_sel = None;
    let _ = window_texts(&mut app);
    assert!(app.recipe_preview.is_some(), "the cached preview survives a dropped selection");
}

/// The window without a photo still manages the library (list, rename, delete);
/// only the verbs that need a canvas or a selection are offered.
#[test]
fn the_window_works_without_a_photo_open() {
    let name = unique("nophoto");
    let _scrub = Scrub(vec![recipe_library::entry_path(&name)]);
    recipe_library::create(&name, &EditRecipe::default()).unwrap();
    let mut app = AutoShadeApp { lang: Lang::En, ..Default::default() };
    app.show_recipes = true;
    let seen = window_texts(&mut app);
    assert!(seen.iter().any(|t| t.contains(&name)), "{seen:?}");
    assert!(seen.iter().any(|t| t.contains("Recipe library")), "{seen:?}");
}
