// One part of the GUI's tests (src/bin/gui/tests.rs includes it): a CPU
// rasteriser for egui's tessellated output, so a panel can be LOOKED AT as a
// PNG without opening a window (the app is never launched while debugging),
// and the scrollbar-gutter pin the 2026-10-12 report asked for.

    /// User report 2026-10-12 (screenshot: the Generative Fill hint running
    /// under the panel's right edge): egui 0.29's floating scrollbar reserves
    /// NO width, so every row filled the panel to its last pixel and the bar
    /// was drawn over the text. With the bar shown — the panel is shorter than
    /// its content — no text may reach the band the bar is drawn in, and the
    /// panel keeps its width while the bar fades in.
    ///
    /// MUTATION THIS CATCHES: `controls_column`'s right gutter set to 0.
    #[test]
    fn no_panel_text_runs_under_the_scrollbar() {
        for lang in [crate::i18n::Lang::En, crate::i18n::Lang::Zh] {
            let mut app = app_with_every_button(lang);
            let theme = crate::theme::ThemePref::Dark;
            let (_, out, panel) = controls_shot(&mut app, theme, sz(1400.0, 900.0), 320.0, 1.0, 12);
            let report = controls_gutter_report(&out, panel, theme);
            assert!(
                report.past.is_empty(),
                "{lang:?}: text reaches the scrollbar band (x > {:.1}): {:#?}",
                report.bar_left,
                report.past
            );
            assert!(
                report.wide.is_empty(),
                "{lang:?}: drawn past the panel's content edge {:.1}: {:#?}",
                report.content_right,
                report.wide
            );
        }
    }

    /// Not a check — the camera. `AUTOSHADE_SHOT_DIR=<dir> cargo test …
    /// panel_screenshots -- --ignored` writes the controls panel in both
    /// languages and both themes at the default and a wide width.
    #[test]
    #[ignore = "writes PNGs for a person to look at"]
    fn panel_screenshots() {
        let dir = std::path::PathBuf::from(std::env::var("AUTOSHADE_SHOT_DIR").expect("AUTOSHADE_SHOT_DIR"));
        std::fs::create_dir_all(&dir).unwrap();
        for lang in [crate::i18n::Lang::En, crate::i18n::Lang::Zh] {
            for theme in crate::theme::ThemePref::ALL {
                for w in [320.0f32, 560.0] {
                    let mut app = app_with_every_button(lang);
                    with_long_analysis(&mut app);
                    let (img, _, panel) = controls_shot(&mut app, theme, sz(w + 40.0, 16_000.0), w, 2.0, 6);
                    let crop = image::imageops::crop_imm(&img, 0, 0, (panel.right() * 2.0) as u32, img.height());
                    crop.to_image().save(dir.join(format!("controls-{lang:?}-{theme:?}-{w}.png"))).unwrap();
                }
                let mut app = app_with_every_button(lang);
                with_long_analysis(&mut app);
                app.workspace_mut().ui_mut().show_analysis_full = true;
                window_shot(&mut app, theme).save(dir.join(format!("analysis-window-{lang:?}-{theme:?}.png"))).unwrap();
            }
        }
    }

    /// An analysis long enough to overflow its box: a verdict, a rationale
    /// sentence with a deterministic note, and the four deep-thinking notes.
    fn with_long_analysis(app: &mut AutoShadeApp) {
        use autoshade::rationale::{keys, render_one, Note};
        let notes = vec![
            Note::new(keys::JUDGE_SCORE, vec![("score", "84".into()), ("critique", "the sky still reads a touch flat".into())]),
            Note::new(keys::THINK_SCENE, vec![("scene", "a red-rock canyon at golden hour, the far wall in shade".into())]),
            Note::new(keys::THINK_LOOK, vec![("look", "warm rock against a clean blue sky, shadows kept open".into())]),
            Note::new(keys::THINK_CRITIQUE, vec![("critique", "close to the target; the highlights could take one more step down".into())]),
            Note::new(keys::PIXEL_TOOLS, vec![("tools", "denoise (grain in the shaded wall)".into())]),
        ];
        let prose = concat!(
            "Exposure lifted half a stop for the shaded wall, highlights pulled to hold the sunlit rim, ",
            "whites eased so the sky keeps its gradient, vibrance up and saturation held so the red rock ",
            "stays believable, a cooler white balance on the sky band only, and a gentle S curve for depth. ",
        );
        app.editor_mut().analysis_mut().rationale = std::iter::once(prose.to_string()).chain(notes.iter().map(render_one)).collect();
        app.editor_mut().doc_mut().recipe.rationale = app.editor().analysis().rationale.clone();
        app.editor_mut().analysis_mut().rationale_notes = notes;
        app.editor_mut().analysis_mut().verdict = Some((autoshade::advisor::Decision::Accept, vec!["balanced".into(), "on target".into()]));
    }

    /// Not a check — the camera for the whole window, both themes:
    /// `AUTOSHADE_SHOT_DIR=<dir> cargo test … window_screenshots -- --ignored`.
    #[test]
    #[ignore = "writes PNGs for a person to look at"]
    fn window_screenshots() {
        let dir = std::path::PathBuf::from(std::env::var("AUTOSHADE_SHOT_DIR").expect("AUTOSHADE_SHOT_DIR"));
        std::fs::create_dir_all(&dir).unwrap();
        for lang in [crate::i18n::Lang::En, crate::i18n::Lang::Zh] {
            for theme in crate::theme::ThemePref::ALL {
                let mut app = app_with_every_button(lang);
                with_long_analysis(&mut app);
                full_window_shot(&mut app, theme, sz(1400.0, 880.0))
                    .save(dir.join(format!("window-{lang:?}-{theme:?}.png")))
                    .unwrap();
            }
        }
    }

    /// The recipe library window over the same window, one shot per language.
    #[test]
    #[ignore = "writes PNGs for a person to look at"]
    fn recipe_library_screenshot() {
        let dir = std::path::PathBuf::from(std::env::var("AUTOSHADE_SHOT_DIR").expect("AUTOSHADE_SHOT_DIR"));
        std::fs::create_dir_all(&dir).unwrap();
        let now = std::time::SystemTime::now();
        // Real library files behind the rows: the details pane reads the entry
        // and develops it, so a screenshot made of synthetic rows would show
        // "unreadable" where the preview belongs.
        let names = ["Warm late sun", "Cool portrait", "DSC02258"];
        let _scrub = Scrub(names.iter().map(|n| autoshade::recipe_library::entry_path(n)).collect());
        let mut mask = autoshade::recipe::LocalAdjustment::default();
        mask.exposure_ev = 0.25;
        let warm = EditRecipe { exposure_ev: 0.35, contrast: 14.0, temperature_k: Some(5200.0), ..Default::default() };
        let _ = autoshade::recipe_library::delete(&names[0]);
        let _ = autoshade::recipe_library::delete(&names[1]);
        let _ = autoshade::recipe_library::delete(&names[2]);
        autoshade::recipe_library::create(&names[0], &EditRecipe { masks: vec![mask.clone()], ..warm.clone() }).unwrap();
        autoshade::recipe_library::create(
            &names[1],
            &EditRecipe { temperature_k: Some(7200.0), tint: -8.0, vibrance: -12.0, ..Default::default() },
        )
        .unwrap();
        autoshade::recipe_library::create(&names[2], &EditRecipe::default()).unwrap();
        let rows = || {
            vec![
                autoshade::recipe_library::Stored {
                    name: names[0].into(),
                    modified: now.checked_sub(std::time::Duration::from_secs(2 * 3600)),
                    masks: 2,
                    error: None,
                },
                autoshade::recipe_library::Stored {
                    name: names[1].into(),
                    modified: now.checked_sub(std::time::Duration::from_secs(6 * 86_400)),
                    masks: 0,
                    error: None,
                },
                autoshade::recipe_library::Stored {
                    name: names[2].into(),
                    modified: now.checked_sub(std::time::Duration::from_secs(200 * 86_400)),
                    masks: 0,
                    error: None,
                },
                autoshade::recipe_library::Stored {
                    name: "Half-written recipe".into(),
                    modified: None,
                    masks: 0,
                    error: Some("expected value at line 1 column 2".into()),
                },
            ]
        };
        for lang in [crate::i18n::Lang::En, crate::i18n::Lang::Zh] {
            let mut app = app_with_every_button(lang);
            // Pixels the preview pane develops (a warm-rock / blue-sky frame,
            // the same stand-in the canvas shots use).
            app.editor_mut().photo_mut().base_preview = Some(std::sync::Arc::new(image::DynamicImage::ImageRgb8(
                image::RgbImage::from_fn(96, 64, |x, y| {
                    if y < 28 {
                        image::Rgb([70 + (x / 2) as u8, 110, 190])
                    } else {
                        image::Rgb([175, 95 + (y / 4) as u8, 55])
                    }
                }),
            )));
            app.workspace_mut().rlib_mut().recipes = rows();
            app.workspace_mut().rlib_mut().recipes_loaded = true;
            app.workspace_mut().rlib_mut().show_recipes = true;
            app.workspace_mut().rlib_mut().recipe_sel = Some(names[0].into());
            full_window_shot(&mut app, crate::theme::ThemePref::Dark, sz(1400.0, 880.0))
                .save(dir.join(format!("recipe-library-{lang:?}.png")))
                .unwrap();
            // …and the empty library, whose state has to invite the first save.
            let mut empty = app_with_every_button(lang);
            empty.workspace_mut().rlib_mut().recipes.clear();
            empty.workspace_mut().rlib_mut().recipes_loaded = true;
            empty.workspace_mut().rlib_mut().show_recipes = true;
            full_window_shot(&mut empty, crate::theme::ThemePref::Dark, sz(1400.0, 880.0))
                .save(dir.join(format!("recipe-library-empty-{lang:?}.png")))
                .unwrap();
        }
    }

