// One part of the GUI's tests (src/bin/gui/tests.rs includes it): Normal's
// four groups in the one workspace's sidebar (panels/normal.rs,
// design-normal-mode.md, design-normal-oneclick.md; 2026-10-23).

    /// The photo with every button armed, with the key state fixed (no key:
    /// the disclosure line is drawn too). The brush is armed on the real
    /// canvas: the one workspace draws its marks.
    fn app_in(lang: Lang) -> AutoShadeApp {
        let mut app = app_with_every_button(lang);
        app.commands().start_mask_brush(None);
        app.workspace_mut().normal_mut().key_present = Some(false);
        app
    }

    /// The one workspace's sidebar draws, top to bottom: the Portraits panel,
    /// Normal's four groups, then the shared AI / Develop / Retouch panels —
    /// and the full toolbar, the library batch rows, the canvas tool marks and
    /// the variant strip are all on the same frame.
    ///
    /// MUTATION THIS CATCHES: dropping any of those panels or rows from
    /// `controls_column` / `upd_top_bar` / `upd_strips_and_side_panels`, or
    /// drawing them out of order.
    #[test]
    fn the_one_workspace_draws_the_portraits_panel_normal_groups_and_shared_panels_in_order() {
        let shots = std::env::var_os("AUTOSHADE_SHOT_DIR").map(PathBuf::from);
        for lang in [Lang::En, Lang::Zh] {
            let mut app = app_in(lang);
            // Portraits with the people found: its panel's four groups open.
            app.editor_mut().doc_mut().recipe.masks.extend(found_masks());
            let texts = controls_panel_texts(&mut app, 1400.0, 20_000.0, 3);
            let at = |key: &str| texts.iter().position(|t| t.starts_with(key));
            // (a) Portraits, (b) Normal's four groups, (c) the shared panels.
            let order = [
                tr(lang, K_PORTRAIT_PEOPLE_AND_BACKGROUND),
                tr(lang, K_NORMAL_ONE_CLICK_LOOK),
                tr(lang, K_NORMAL_FINE_TUNE),
                tr(lang, K_NORMAL_NOISE),
                tr(lang, K_NORMAL_SAVE),
                tr(lang, K_AI_AI_GENERATION_REVERSE_FIT),
                tr(lang, K_DEVELOP_TONE_WB),
            ];
            let mut last = None;
            for key in order {
                let here = at(key).unwrap_or_else(|| panic!("{lang:?}: the sidebar draws {key:?}"));
                assert!(last.is_none_or(|l| here > l), "{lang:?}: {key:?} is out of order");
                last = Some(here);
            }
            // Normal's controls are all reachable on that same sidebar.
            for key in [
                tr(lang, K_NORMAL_NATURAL),
                tr(lang, K_NORMAL_BRIGHT_CLEAN),
                tr(lang, K_NORMAL_WARM),
                tr(lang, K_NORMAL_COOL_NIGHT),
                tr(lang, K_NORMAL_FILM),
                tr(lang, K_NORMAL_BLACK_WHITE),
                tr(lang, K_NORMAL_MATCH_MY_REFERENCE_PHOTOS),
                tr(lang, K_NORMAL_SAVE_AS_JPG),
                tr(lang, K_NORMAL_APPLY_THIS_LOOK_TO_EVERY_PHOTO_IN_THE_FOLDER),
            ] {
                assert!(texts.iter().any(|t| t == key), "{lang:?}: {key:?}");
            }
            // …and the full window's toolbar, library rows, canvas marks and
            // variant strip are on, on the same photo.
            let (texts, drawn, img) = window_frame(&mut app, 1400.0, 1000.0);
            let labels: Vec<&str> = drawn.iter().map(|d| d.label.as_str()).collect();
            let has_text = |s: &str| texts.iter().any(|x| x == s);
            let has_button = |s: &str| labels.contains(&s);
            for keep in [
                tr(lang, K_APP_RESET),
                tr(lang, K_APP_TURN_LEFT),
                tr(lang, K_APP_TURN_RIGHT),
                tr(lang, K_APP_EXPORT),
                tr(lang, K_APP_SAVE_DEVELOP),
                "⌨",
            ] {
                assert!(has_button(keep), "{lang:?}: the toolbar keeps {keep:?}");
            }
            for keep in [tr(lang, K_APP_COMPARE), tr(lang, K_APP_SINGLE)] {
                assert!(has_text(keep), "{lang:?}: both view switches: {keep:?}");
            }
            for keep in [
                tr(lang, K_GALLERY_COPY_RECIPE).to_string(),
                trf(lang, K_GALLERY_PASTE_TO_SELECTED, &[("n", "1")]),
                trf(lang, K_GALLERY_RENDER_SELECTED, &[("n", "1")]),
            ] {
                assert!(has_button(&keep), "{lang:?}: the library keeps {keep:?}");
            }
            for keep in [tr(lang, K_CANVAS_FIT), "1:1"] {
                assert!(has_button(keep), "{lang:?}: the canvas keeps {keep:?}");
            }
            assert!(
                texts.iter().any(|x| x.ends_with('%') && x[..x.len() - 1].parse::<f32>().is_ok()),
                "{lang:?}: the zoom readout"
            );
            assert_eq!(app.workspace().masks().paint_mode, true, "{lang:?}: the armed brush stays armed");
            assert!(has_text(&app.workspace().status()), "{lang:?}: the status bar is in the frame's texts");
            assert!(has_text(tr(lang, K_GUI_ORIGINAL__2)), "{lang:?}: the variant strip");
            if let Some(dir) = &shots {
                std::fs::create_dir_all(dir).unwrap();
                img.save(dir.join(format!("window-one-workspace-{lang:?}.png"))).unwrap();
            }
        }
    }

    /// Every text Normal's four groups draw on their own, in one tall frame.
    fn normal_panel_texts(app: &mut AutoShadeApp) -> Vec<String> {
        let mut ctx = harness::Ctx::new();
        crate::theme::install_fonts(&ctx);
        crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
        let mut out = None;
        for i in 0..3 {
            out = Some(ctx.run_with(420.0, 4000.0, Some(i as f64), |ctx| {
                harness::central_panel(ctx, |ui| app.commands().normal_panel(ui));
            }));
        }
        out.unwrap().texts()
    }

    /// Design §5: 「没有一个术语」 — every text Normal's four groups draw, in
    /// both languages, is free of the workspace's trade words. The shared
    /// panels around them in the same sidebar do use those words.
    ///
    /// MUTATION THIS CATCHES: word a Normal control with a trade term (a
    /// "recipe", a "develop", a "mask") and a line below fails; drawing only
    /// the groups keeps the check about Normal's own words.
    #[test]
    fn the_normal_groups_say_no_trade_words_in_either_language() {
        let words = ["配方", "显影", "反推", "变体", "蒙版", "API", "recipe", "develop", "reverse-fit", "variant", "mask"];
        for lang in [Lang::En, Lang::Zh] {
            let mut app = app_in(lang);
            let texts = normal_panel_texts(&mut app);
            assert!(texts.len() > 30, "{lang:?}: the four groups were drawn ({} texts)", texts.len());
            for text in &texts {
                let low = text.to_lowercase();
                for w in words {
                    let hit = if w.is_ascii() && w != "API" { low.contains(w) } else { text.contains(w) };
                    assert!(!hit, "{lang:?}: {text:?} says {w:?}");
                }
            }
        }
    }

    /// A stand-in for the paid chain: the no-key path's own proposer
    /// (`advisor::HeuristicProposer`, what `produce_recipe` falls back to
    /// without an image key) over a dark frame, verified Accept.
    fn heuristic_runner(
        _photo: &std::path::Path,
        req: &autoshade::pipeline::GradeRequest,
    ) -> anyhow::Result<(EditRecipe, autoshade::advisor::Verdict, Vec<autoshade::rationale::Note>)> {
        let mut luma = vec![0u32; 256];
        luma[40] = 1000;
        let hist = autoshade::decode::Histogram {
            luma: luma.clone(),
            r: luma.clone(),
            g: luma.clone(),
            b: luma,
            clip_black_pct: 0.0,
            clip_white_pct: 0.0,
            sample_pixels: 1000,
        };
        let (recipe, note) = autoshade::advisor::HeuristicProposer::default().propose_noted(&hist, req.strength)?;
        let verdict = autoshade::advisor::Verdict {
            decision: autoshade::advisor::Decision::Accept,
            reasons: Vec::new(),
            revised_hint: None,
        };
        Ok((recipe, verdict, vec![note]))
    }

    /// Poll the app's workers until the one-click's analysis has landed.
    fn land_analysis(app: &mut AutoShadeApp, ctx: &harness::Ctx) {
        let start = std::time::Instant::now();
        while app.workspace().tools().analyze_inflight {
            assert!(start.elapsed() < std::time::Duration::from_secs(20), "the analysis never landed");
            std::thread::sleep(std::time::Duration::from_millis(20));
            app.poll_workers(ctx);
        }
    }

    /// A photo the store keys by path (nothing reads the file), its develop
    /// folder scrubbed afterwards.
    fn normal_photo(tag: &str) -> (AutoShadeApp, PathBuf, Scrub) {
        let dir = std::env::temp_dir().join(format!("autoshade-gui-normal-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join(format!("_gui_normal_{tag}.jpg"));
        let dev = autoshade::store::develop_dir(&src);
        let _ = std::fs::remove_dir_all(&dev);
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {
     src_path: Some(src.clone()),
     ..Default::default()
 }; editor });
        app.workspace_mut().normal_mut().runner = Some(heuristic_runner);
        (app, src, Scrub(vec![dir, dev]))
    }

    /// A preset card's click builds a DIRECTION request carrying that
    /// preset's text, firm adherence, no library, no reference photos, no
    /// deep thinking, at the look strength — every field the design fixes.
    ///
    /// MUTATION THIS CATCHES: any field of `logic::look::plan`'s preset arm
    /// changed (the guide, the text, the adherence, the embed switch, the
    /// reference photos, the strength's source) fails an assert below.
    #[test]
    fn a_preset_click_asks_for_that_presets_direction() {
        use crate::buttons::DRAWN;
        let (mut app, _src, _scrub) = normal_photo("preset");
        app.aip_mut().grade_strength = 0.4;
        let mut ctx = harness::Ctx::new();
        crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
        let size = sz(1400.0, 6000.0);
        let mut frame = 0;
        app.workspace_mut().normal_mut().key_present = Some(true);
        ctx.frame(&mut app, size, frame, Vec::new());
        frame += 1;
        DRAWN.with_borrow_mut(Vec::clear);
        ctx.frame(&mut app, size, frame, Vec::new());
        frame += 1;
        let mono = DRAWN.with_borrow(|d| d.iter().find(|b| b.label == "Black & white").map(|b| (b.rect, b.cell)));
        let (rect, in_cell) = mono.expect("the Black & white card is drawn");
        assert!(in_cell, "a preset card is a grid cell");
        ctx.click(&mut app, size, &mut frame, harness::rect(rect).center());
        let (req, text, library) = app.workspace_mut().normal_mut().last_request.clone().expect("the click started an analysis");
        assert_eq!(req.guide, autoshade::pipeline::Guide::Direction);
        assert_eq!(text.as_deref(), autoshade::advisor::presets::preset_guidance("mono"));
        assert!(text.unwrap().contains("set convert_to_grayscale to true"));
        assert_eq!(req.adherence.get(), crate::logic::look::PRESET_ADHERENCE);
        assert!(req.adherence.get() > 0.7, "a firm brief");
        assert!(!req.send_reference_image && !req.think && !req.embed.on());
        assert_eq!(req.library_style(), 0.0, "the library is not read");
        assert_eq!(req.strength.get(), 0.4, "the look strength");
        assert_eq!(library, autoshade::library::NORMAL_NAME);
        land_analysis(&mut app, &ctx);
    }

    /// The folder chain: a built Normal library hands over to the analysis
    /// at once, and the request is the design's — guided by the LIBRARY (its
    /// own, `library::NORMAL_NAME`), Style 1.0 (reproduce the references),
    /// the reference photos sent along, the search on. A failed build says
    /// why and asks nothing.
    ///
    /// MUTATION THIS CATCHES: flip the guide, lower the style below 0.85,
    /// turn `send_reference_image` off, pass the default library, or drop the
    /// hand-off in `on_one_click_built` — each fails an assert below.
    #[test]
    fn the_folder_chain_builds_the_library_request() {
        let (mut app, src, _scrub) = normal_photo("folder");
        let ctx = harness::Ctx::new();
        // What the reference-build action leaves while the folder is read.
        let building = |app: &mut AutoShadeApp| {
            app.workspace_mut().normal_mut().building = true;
            app.workspace_mut().normal_mut().build_photo = Some(src.clone());
            app.workspace_mut().set_busy(true);
        };
        building(&mut app);
        app.commands().on_one_click_built(Lang::En, Err("no photos in that folder".into()));
        assert!(app.workspace_mut().normal_mut().last_request.is_none(), "a failed build asks nothing");
        assert!(!app.workspace_mut().normal_mut().building);
        assert!(matches!(&app.workspace_mut().normal_mut().line, crate::panels::normal::NormalLine::Failed(t) if t.contains("no photos")));
        building(&mut app);
        app.commands().on_one_click_built(Lang::En, Ok(3));
        let (req, text, library) = app.workspace_mut().normal_mut().last_request.clone().expect("the build handed over to the analysis");
        assert_eq!(req.guide, autoshade::pipeline::Guide::Library);
        assert_eq!(req.style, 1.0);
        assert!(req.style >= autoshade::library::STYLE_TARGET_MIN, "the references are the look to reproduce");
        assert!(req.send_reference_image, "the reference photos go along");
        assert!(req.embed.on(), "the library is searched");
        assert!(!req.think);
        assert_eq!(text, None, "no typed direction");
        assert_eq!(library, autoshade::library::NORMAL_NAME, "Normal's own library");
        assert_eq!(app.workspace_mut().normal_mut().line, crate::panels::normal::NormalLine::Working);
        land_analysis(&mut app, &ctx);
    }

    /// With no image key the chain still lands: the built-in proposer's
    /// recipe reaches `on_analyzed`, goes into the sliders, is undoable, and
    /// the one-click line says the built-in look was used.
    #[test]
    fn without_a_key_the_one_click_lands_the_built_in_look() {
        let (mut app, _src, _scrub) = normal_photo("nokey");
        let ctx = harness::Ctx::new();
        assert_eq!(app.editor().doc().recipe.exposure_ev, 0.0);
        app.commands().apply_actions(vec![logic::Action::Look(logic::look::LookIntent::of(
            logic::look::LookKind::OneClick(crate::logic::look::OneClick::Preset("natural")),
        ))]);
        assert!(app.workspace().busy() && app.workspace().tools().analyze_inflight);
        land_analysis(&mut app, &ctx);
        assert!(!app.workspace().busy());
        assert!(app.editor().doc().recipe.exposure_ev > 0.0, "the dark frame was brightened: {}", app.editor().doc().recipe.exposure_ev);
        assert_eq!(app.workspace_mut().normal_mut().line, crate::panels::normal::NormalLine::BuiltIn);
        assert_eq!(app.workspace().status(), "No AI key set — built-in look used");
        assert_eq!(app.workspace_mut().normal_mut().key_present, Some(false));
        let (photo, kept) = app.workspace_mut().normal_mut().result.clone().expect("the one-click result is remembered");
        assert_eq!(Some(photo), app.editor().photo().src_path);
        assert_eq!(kept.exposure_ev, app.editor().doc().recipe.exposure_ev);
        // Undoable, like every analysis landing.
        { let (e, w, _l) = app.editor_workspace_mut(); e.commit_now(&*w) };
        { let (e, w, l) = app.editor_workspace_mut(); e.undo(w, l, &ctx) };
        assert_eq!(app.editor().doc().recipe.exposure_ev, 0.0, "Ctrl+Z goes back");
    }

    /// Poll the app's workers until nothing is running.
    fn land_all(app: &mut AutoShadeApp, ctx: &harness::Ctx) {
        let start = std::time::Instant::now();
        while app.workspace().busy() {
            assert!(start.elapsed() < std::time::Duration::from_secs(60), "the worker never landed");
            std::thread::sleep(std::time::Duration::from_millis(20));
            app.poll_workers(ctx);
        }
    }

    /// The workspace's save keys keep their meaning: Ctrl+S saves the develop
    /// and Ctrl+E exports. Normal's own jpg save is its sidebar button
    /// (`start_normal_save`), not a key.
    ///
    /// MUTATION THIS CATCHES: route either key to `start_normal_save` in
    /// `upd_shortcuts` and a Save-group jpg delivery starts instead of the
    /// workspace's save / export.
    #[test]
    fn the_workspace_save_keys_save_the_develop_and_export() {
        let press = |app: &mut AutoShadeApp, ctx: &mut harness::Ctx, k: harness::Key| {
            ctx.run_default_events(vec![harness::Event::KeyCommand(k)], |ctx| app.upd_shortcuts(ctx));
        };
        // Ctrl+E exports into the destination setting's folder.
        let (mut app, src, _scrub) = normal_photo("keys-export");
        image::RgbImage::from_pixel(32, 24, image::Rgb([120, 90, 60])).save(&src).unwrap();
        let out = std::env::temp_dir().join(format!("autoshade-gui-normal-keys-export-out-{}", std::process::id()));
        let _scrub_out = Scrub(vec![out.clone()]);
        app.workspace_mut().exp_mut().dest = ExportDest::LastUsed;
        app.workspace_mut().exp_mut().last_dir = Some(out.clone());
        let mut ctx = harness::Ctx::new();
        press(&mut app, &mut ctx, harness::Key::E);
        assert!(app.workspace().busy(), "the workspace export started");
        assert!(!app.workspace_mut().normal_mut().delivering, "not the Save group's delivery");
        assert!(app.workspace().status().contains(&out.file_name().unwrap().to_string_lossy().into_owned()), "{}", app.workspace().status());
        land_all(&mut app, &ctx);
        // Ctrl+S saves the develop: the shared ● marker goes clean.
        let (mut app, src, _scrub) = normal_photo("keys-save");
        image::RgbImage::from_pixel(32, 24, image::Rgb([60, 90, 120])).save(&src).unwrap();
        let b0 = std::sync::Arc::new(image::DynamicImage::new_rgba8(4, 4));
        app.editor_mut().doc_mut().variants = vec![Variant {
            id: ORIGINAL_VARIANT_ID.into(),
            name: None,
            kind: VariantKind::Original,
            recipe: EditRecipe::default(),
            base: Some(b0.clone()),
            origin: None,
            thumb: None,
        }];
        app.editor_mut().doc_mut().active = 0;
        app.editor_mut().photo_mut().base_preview = Some(b0);
        { let (e, w, _l) = app.editor_workspace_mut(); e.reset_history(&*w) };
        app.editor_mut().doc_mut().recipe.exposure_ev = 0.4;
        assert!(app.editor().unsaved_marker_dirty(), "the edit is unsaved before Ctrl+S");
        let mut ctx = harness::Ctx::new();
        press(&mut app, &mut ctx, harness::Key::S);
        assert!(!app.workspace_mut().normal_mut().delivering, "not the Save group's delivery");
        assert!(!app.workspace().busy());
        assert!(!app.editor().unsaved_marker_dirty(), "the develop was saved: {}", app.workspace().status());
    }

    /// 「Apply this look to every photo in the folder」 settles the jpgs'
    /// folder before anything is pasted: a cancelled folder dialog changes no
    /// photo and leaves the status line empty; a picked folder is carried to
    /// the render.
    ///
    /// MUTATION THIS CATCHES: keep the old status on cancel, or paste before
    /// asking (`apply_all` would not carry the folder), and an assert fails.
    #[test]
    fn apply_to_every_photo_asks_for_the_folder_before_it_pastes() {
        let (mut app, src, _scrub) = normal_photo("applyall");
        let ctx = harness::Ctx::new();
        app.workspace_mut().gal_mut().gallery = vec![src.clone()];
        app.workspace_mut().exp_mut().dest = ExportDest::Ask;
        app.workspace_mut().set_status("Recipe pasted onto 1 photo".into());
        app.workspace_mut().normal_mut().folder_pick = Some(None);
        app.commands().start_apply_to_folder();
        assert_eq!(app.workspace().status(), "", "a cancelled dialog leaves nothing on the line");
        assert!(!app.workspace().busy() && app.workspace_mut().normal_mut().apply_all.is_none(), "and nothing was pasted");
        let out = std::env::temp_dir().join(format!("autoshade-gui-normal-applyall-out-{}", std::process::id()));
        let _scrub_out = Scrub(vec![out.clone()]);
        app.workspace_mut().normal_mut().folder_pick = Some(Some(out.clone()));
        app.commands().start_apply_to_folder();
        assert!(app.workspace().busy(), "the paste runs");
        assert_eq!(app.workspace_mut().normal_mut().apply_all.as_deref(), Some(out.as_path()), "into the folder picked first");
        land_all(&mut app, &ctx);
        assert!(app.workspace_mut().normal_mut().apply_all.is_none());
    }

    /// The Save group says where the jpg goes, in plain words: never beside
    /// the photo — the app's output folder (Settings), the folder last saved
    /// to, or a question each time.
    #[test]
    fn the_save_line_names_where_the_jpg_goes() {
        for lang in [Lang::En, Lang::Zh] {
            let mut app = app_in(lang);
            app.workspace_mut().exp_mut().dest = ExportDest::LastUsed;
            app.workspace_mut().exp_mut().last_dir = Some(std::env::temp_dir().join("Deliveries"));
            let texts = normal_panel_texts(&mut app);
            let last = trf(lang, K_NORMAL_SAVES_TO_THE_FOLDER_YOU_LAST_SAVED_TO, &[("folder", "Deliveries")]);
            assert!(texts.contains(&last), "{lang:?}: {last:?}");
            app.workspace_mut().exp_mut().dest = ExportDest::OutFolder;
            let texts = normal_panel_texts(&mut app);
            let root = autoshade::config::delivery_root();
            let name = root.file_name().map_or_else(|| abs_display(&root), |n| n.to_string_lossy().into_owned());
            let out = trf(lang, K_NORMAL_SAVES_TO_THE_APP_S_OUTPUT_FOLDER_CHANGE_IT_IN_SE, &[("folder", &name)]);
            assert!(texts.contains(&out), "{lang:?}: {out:?}");
            app.workspace_mut().exp_mut().dest = ExportDest::Ask;
            let texts = normal_panel_texts(&mut app);
            assert!(texts.iter().any(|t| t == tr(lang, K_NORMAL_ASKS_WHERE_TO_SAVE_EACH_TIME)), "{lang:?}: ask");
        }
    }

    /// The words around the noise switch say, plainly, that it takes a
    /// moment and the preview then shows what gets saved (never "applied at
    /// save" any more); the first-download size follows while the model is
    /// not known to be on disk and the photo is a rendered image; without the
    /// helper the no-tool line stands alone. None of them says a trade word.
    #[test]
    fn the_noise_lines_say_it_takes_a_moment_and_the_first_run_downloads_the_model() {
        let words = ["配方", "显影", "反推", "变体", "蒙版", "API", "recipe", "develop", "reverse-fit", "variant", "mask", "sidecar"];
        for lang in [Lang::En, Lang::Zh] {
            let plain = tr(lang, K_NORMAL_IT_TAKES_A_MOMENT_THE_PREVIEW_THEN_SHOWS_THE_RES).to_string();
            let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {
     src_path: Some(PathBuf::from("phone.jpg")),
     ..Default::default()
 }; editor }).but_lang(lang);
            assert_eq!(app.editor().noise_lines(&app.workspace(), lang, false), vec![tr(lang, K_NORMAL_THIS_DOWNLOAD_DOES_NOT_INCLUDE_THE_NOISE_TOOL).to_string()]);
            app.workspace_mut().normal_mut().denoise_ready = Some(true);
            assert_eq!(app.editor().noise_lines(&app.workspace(), lang, true), vec![plain.clone()], "{lang:?}: the model is there");
            for ready in [Some(false), None] {
                app.workspace_mut().normal_mut().denoise_ready = ready;
                let lines = app.editor().noise_lines(&app.workspace(), lang, true);
                assert_eq!(lines.len(), 2, "{lang:?} {ready:?}: {lines:?}");
                assert_eq!(lines[0], plain);
                assert!(lines[1].contains("72 MB"), "{lang:?}: {lines:?}");
            }
            app.editor_mut().photo_mut().src_path = Some(PathBuf::from("camera.arw"));
            assert_eq!(app.editor().noise_lines(&app.workspace(), lang, true), vec![plain.clone()], "{lang:?}: a RAW downloads nothing new");
            let (moment, preview) = match lang {
                Lang::En => ("takes a moment", "preview"),
                Lang::Zh => ("稍等", "预览"),
            };
            assert!(plain.contains(moment) && plain.contains(preview), "{lang:?}: {plain}");
            let mut said = app.editor().noise_lines(&app.workspace(), lang, true);
            app.editor_mut().photo_mut().src_path = Some(PathBuf::from("phone.jpg"));
            said.extend(app.editor().noise_lines(&app.workspace(), lang, true));
            said.extend(
                [
                    tr(lang, K_NORMAL_CLEANS_THE_GRAIN_OUT_OF_THE_PHOTO_AND_THE_PREVIE),
                    tr(lang, K_ACTIONS_REMOVING_THE_GRAIN_THIS_TAKES_A_MOMENT),
                    tr(lang, K_ACTIONS_AI_DENOISE_DONE_TURN_THE_SWITCH_OFF_TO_SEE_THE_P),
                    tr(lang, K_ACTIONS_AI_DENOISE_ON),
                    tr(lang, K_ACTIONS_AI_DENOISE_OFF),
                ]
                .map(str::to_string),
            );
            for text in &said {
                assert!(!text.contains("when you save") && !text.contains("保存时处理"), "{lang:?}: {text:?}");
                let low = text.to_lowercase();
                for w in words {
                    let hit = if w.is_ascii() && w != "API" { low.contains(w) } else { text.contains(w) };
                    assert!(!hit, "{lang:?}: {text:?} says {w:?}");
                }
            }
        }
    }

    /// One AI denoise, not two: Normal's switch IS the Retouch panel's
    /// (`start_ai_denoise`) — it meets that door's own gate, runs its worker
    /// and lands through its message (`Msg::Retouched` with a ◈ card) — so
    /// the canvas shows the result and the save renders that card. Off goes
    /// back to the card it was made from; on again shows the ◈ card already
    /// made with no second run; the look the sliders made rides across both
    /// ways.
    ///
    /// MUTATION THIS CATCHES: call anything but `start_ai_denoise` on the
    /// way on (the turned-photo gate and the worker's own failure are not
    /// met), drop the `after_normal_denoise` hook (the ◈ card is not
    /// remembered and on-again runs a second time), switch to the wrong card
    /// on the way off, or bring back the export-time denoise in
    /// `start_normal_save`.
    #[test]
    fn the_noise_switch_is_the_retouch_denoise_and_round_trips() {
        use crate::panels::normal::DenoisePair;
        let (mut app, src, _scrub) = normal_photo("denoise");
        let ctx = harness::Ctx::new();
        let b0 = std::sync::Arc::new(image::DynamicImage::new_rgba8(4, 4));
        app.editor_mut().doc_mut().variants = vec![Variant {
            id: ORIGINAL_VARIANT_ID.into(),
            name: None,
            kind: VariantKind::Original,
            recipe: EditRecipe::default(),
            base: Some(b0.clone()),
            origin: None,
            thumb: None,
        }];
        app.editor_mut().doc_mut().active = 0;
        app.editor_mut().photo_mut().base_preview = Some(b0);
        { let (e, w, _l) = app.editor_workspace_mut(); e.reset_history(&*w) };
        // The Retouch door's own gate answers: a turned photo is refused
        // before any worker, with that door's words.
        app.editor_mut().doc_mut().recipe.quarter_turns = 1;
        app.commands().set_normal_denoise(true);
        assert!(!app.workspace().busy(), "refused before a worker");
        assert!(app.workspace().status().contains("this photo is turned"), "{}", app.workspace().status());
        assert!(app.workspace_mut().normal_mut().denoise_pair.is_none(), "a refused run remembers nothing");
        app.editor_mut().doc_mut().recipe.quarter_turns = 0;
        // The door's worker runs: this photo has no file, so it fails fast at
        // the decode — through `Msg::Retouched`, the Retouch button's message.
        app.commands().set_normal_denoise(true);
        assert!(app.workspace().busy(), "the denoise worker runs: {}", app.workspace().status());
        assert_eq!(app.workspace().status(), tr(Lang::En, K_ACTIONS_REMOVING_THE_GRAIN_THIS_TAKES_A_MOMENT));
        assert!(app.editor().normal_denoise_on(&app.workspace()), "the switch reads on while it runs");
        land_all(&mut app, &ctx);
        assert!(!app.editor().normal_denoise_on(&app.workspace()), "a failed run leaves the switch off");
        assert_eq!(app.editor().doc().variants.len(), 1, "and no card");
        // A landed run, through the same message the worker sends.
        app.workspace_mut().normal_mut().denoise_pair =
            Some(DenoisePair { photo: src.clone(), plain: ORIGINAL_VARIANT_ID.into(), denoised: None });
        let master = std::path::PathBuf::from("out/_gui_normal_denoise.denoise.png");
        let note = RetouchNote::Denoised { out: master.clone(), on_mosaic: false };
        let done = Ok((image::DynamicImage::new_rgba8(4, 4), note, master.clone(), RetouchKind::NewDenoised));
        app.services().tx.send(Msg::Retouched(app.workspace().tools().gen_epoch, Box::new(done))).unwrap();
        app.poll_workers(&ctx);
        assert_eq!(app.editor().doc().variants[app.editor().doc().active].kind, VariantKind::Denoised, "the ◈ card is on the canvas");
        assert_eq!(app.editor().active_source_path().as_deref(), Some(master.as_path()), "…and is what a save renders");
        assert!(app.editor().normal_denoise_on(&app.workspace()));
        let made = app.editor().doc().variants[app.editor().doc().active].id.clone();
        assert_eq!(app.workspace_mut().normal_mut().denoise_pair.as_ref().and_then(|d| d.denoised.clone()), Some(made.clone()), "remembered");
        assert_eq!(app.workspace().status(), tr(Lang::En, K_ACTIONS_AI_DENOISE_DONE_TURN_THE_SWITCH_OFF_TO_SEE_THE_P));
        // Off: the card it was made from, the look kept.
        app.editor_mut().doc_mut().recipe.exposure_ev = 0.7;
        app.commands().set_normal_denoise(false);
        assert!(!app.workspace().busy());
        assert_eq!(app.editor().doc().variants[app.editor().doc().active].id, ORIGINAL_VARIANT_ID, "back to the un-denoised card");
        assert_eq!(app.editor().active_source_path().as_deref(), Some(src.as_path()));
        assert_eq!(app.editor().doc().recipe.exposure_ev, 0.7, "the look rides across");
        assert!(!app.editor().normal_denoise_on(&app.workspace()));
        assert_eq!(app.workspace().status(), tr(Lang::En, K_ACTIONS_AI_DENOISE_OFF));
        // On again: the same ◈ card, no second run.
        app.editor_mut().doc_mut().recipe.contrast = 12.0;
        app.commands().set_normal_denoise(true);
        assert!(!app.workspace().busy(), "no second sidecar run");
        assert_eq!(app.editor().doc().variants.len(), 2, "no second card");
        assert_eq!(app.editor().doc().variants[app.editor().doc().active].id, made, "the master already made");
        assert_eq!((app.editor().doc().recipe.exposure_ev, app.editor().doc().recipe.contrast), (0.7, 12.0), "the look rides across");
        assert_eq!(app.workspace().status(), tr(Lang::En, K_ACTIONS_AI_DENOISE_ON));
        // The switch's behaviour above IS the claim: turning it on runs the
        // Retouch worker (busy + that door's status), turning it off returns to
        // the un-denoised card, and the save path renders whatever card is
        // active with no denoise of its own (the save line's own test drives
        // the render). No source text needs pinning for it.
    }

    /// A second save into a name the app made never replaces the first and
    /// is never refused: it gets a number — `-2`, then `-3` — claimed on disk
    /// (`claim_numbered`), and the finished line names the file actually
    /// written. A file the user picks in the save window (which confirms a
    /// replace itself) is theirs: it is replaced, not numbered — and the next
    /// app-made name is numbered again. 「Apply to every photo」 into a folder
    /// already saved once writes every photo as `-2`, and says how many got a
    /// number. The workspace's render path keeps the library's refusal to
    /// replace a photo outside its output areas.
    ///
    /// MUTATION THIS CATCHES: drop `OutName::Numbered` on either Normal path
    /// (the guard refuses the second save), number the picked file
    /// (`OutName::Chosen` → `Numbered`), claim without `create_new` stepping
    /// (the first jpg is replaced), or name the bare file in the landing,
    /// and an assert below fails.
    #[test]
    fn a_second_sidebar_save_gets_a_number_and_never_replaces_the_first() {
        let (mut app, src, _scrub) = normal_photo("numbered");
        image::RgbImage::from_pixel(32, 24, image::Rgb([60, 90, 120])).save(&src).unwrap();
        let src2 = src.with_file_name("_gui_normal_numbered_b.jpg");
        image::RgbImage::from_pixel(32, 24, image::Rgb([90, 60, 120])).save(&src2).unwrap();
        let _scrub2 = Scrub(vec![autoshade::store::develop_dir(&src2)]);
        let out = std::env::temp_dir().join(format!("autoshade-gui-normal-numbered-out-{}", std::process::id()));
        let _scrub_out = Scrub(vec![out.clone()]);
        app.workspace_mut().exp_mut().dest = ExportDest::LastUsed;
        app.workspace_mut().exp_mut().last_dir = Some(out.clone());
        let ctx = harness::Ctx::new();
        let names = ["_gui_normal_numbered.developed.jpg", "_gui_normal_numbered.developed-2.jpg", "_gui_normal_numbered.developed-3.jpg"];
        let mut sizes = Vec::new();
        for (i, name) in names.iter().enumerate() {
            app.editor_mut().doc_mut().recipe.exposure_ev = 0.2 * (i + 1) as f32;
            app.commands().start_normal_save();
            assert!(app.workspace().busy(), "save {}: {}", i + 1, app.workspace().status());
            assert!(app.workspace().status().contains(name), "save {}: the line names the file: {}", i + 1, app.workspace().status());
            land_all(&mut app, &ctx);
            let folder = out.file_name().unwrap().to_string_lossy().into_owned();
            assert_eq!(app.workspace().status(), trf(Lang::En, K_ACTIONS_SAVED_TO, &[("file", name), ("folder", &folder)]));
            sizes.push(std::fs::metadata(out.join(name)).map(|m| m.len()).unwrap_or(0));
        }
        assert!(sizes.iter().all(|&n| n > 0), "three real jpgs, none replaced by the next: {sizes:?}");
        // A file picked in the save window: replaced, not numbered.
        let picked = out.join(names[0]);
        let before = std::fs::read(&picked).unwrap();
        app.workspace_mut().exp_mut().dest = ExportDest::Ask;
        app.workspace_mut().normal_mut().file_pick = Some(Some(picked.clone()));
        app.editor_mut().doc_mut().recipe.exposure_ev = -1.5;
        app.commands().start_normal_save();
        assert!(app.workspace().busy(), "the picked save runs: {}", app.workspace().status());
        land_all(&mut app, &ctx);
        let folder = out.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(app.workspace().status(), trf(Lang::En, K_ACTIONS_SAVED_TO, &[("file", names[0]), ("folder", &folder)]));
        assert_ne!(std::fs::read(&picked).unwrap(), before, "the picked file was replaced");
        let fourth = out.join("_gui_normal_numbered.developed-4.jpg");
        assert!(!fourth.exists(), "a picked file is never numbered");
        // …while the app-made name is numbered again.
        app.workspace_mut().exp_mut().dest = ExportDest::LastUsed;
        app.workspace_mut().exp_mut().last_dir = Some(out.clone());
        app.editor_mut().doc_mut().recipe.exposure_ev = 0.9;
        app.commands().start_normal_save();
        land_all(&mut app, &ctx);
        assert!(fourth.is_file(), "the app-made name steps on to -4: {}", app.workspace().status());
        // The whole folder, twice: the second run numbers every photo.
        app.workspace_mut().gal_mut().gallery = vec![src.clone(), src2.clone()];
        let all = std::env::temp_dir().join(format!("autoshade-gui-normal-numbered-all-{}", std::process::id()));
        let _scrub_all = Scrub(vec![all.clone()]);
        app.workspace_mut().exp_mut().last_dir = Some(all.clone());
        for run in 0..2 {
            app.editor_mut().doc_mut().recipe.contrast = 10.0 * (run + 1) as f32;
            app.commands().start_apply_to_folder();
            land_all(&mut app, &ctx);
            let folder = all.file_name().unwrap().to_string_lossy().into_owned();
            let saved = trf(Lang::En, K_ACTIONS_PHOTOS_SAVED_TO, &[("n", "2"), ("folder", &folder)]);
            if run == 0 {
                assert_eq!(app.workspace().status(), saved);
            } else {
                let numbered = trf(Lang::En, K_ACTIONS_OF_THEM_HAVE_A_NUMBER_ADDED_TO_THE_NAME_SO_NO_EA, &[("k", "2")]);
                assert_eq!(app.workspace().status(), format!("{saved}. {numbered}"));
            }
        }
        let mut written: Vec<String> =
            std::fs::read_dir(&all).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        written.sort();
        assert_eq!(
            written,
            [
                "_gui_normal_numbered.developed-2.jpg",
                "_gui_normal_numbered.developed.jpg",
                "_gui_normal_numbered_b.developed-2.jpg",
                "_gui_normal_numbered_b.developed.jpg",
            ],
            "every photo once bare, once with -2"
        );
        // The workspace's render path keeps the library's refusal (outside
        // its output areas).
        app.commands().start_render_to(out.join(names[0]));
        assert!(!app.workspace().busy());
        assert!(app.workspace().status().contains("refusing to overwrite"), "{}", app.workspace().status());
    }

    /// The guard protects FILES, not folders (user ruling 2026-11-14): a save
    /// into the photo's own folder lands under a fresh name, and only a path
    /// that would REPLACE an existing photo is refused.
    ///
    /// MUTATION THIS CATCHES: put the folder refusal back and the save never
    /// renders; drop the overwrite refusal and the existing photo is replaced.
    #[test]
    fn a_save_into_the_photos_own_folder_lands_and_replacing_a_photo_is_refused() {
        let (mut app, src, _scrub) = normal_photo("ownfolder");
        image::RgbImage::from_pixel(32, 24, image::Rgb([60, 90, 120])).save(&src).unwrap();
        let own = src.parent().unwrap().to_path_buf();
        app.workspace_mut().exp_mut().dest = ExportDest::LastUsed;
        app.workspace_mut().exp_mut().last_dir = Some(own.clone());
        let ctx = harness::Ctx::new();
        app.editor_mut().doc_mut().recipe.exposure_ev = 0.3;
        app.commands().start_normal_save();
        land_all(&mut app, &ctx);
        let landed: Vec<String> = std::fs::read_dir(&own)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".jpg"))
            .collect();
        assert!(
            landed.iter().any(|n| n.starts_with("_gui_normal_ownfolder") && n.ends_with(".jpg")),
            "the jpg landed beside the photo: {landed:?} / {}",
            app.workspace().status()
        );
        // The photo itself is never replaced: a render aimed at an existing
        // image is refused, and that file keeps its bytes.
        let victim = own.join("_gui_normal_ownfolder_existing.jpg");
        image::RgbImage::from_pixel(32, 24, image::Rgb([10, 20, 30])).save(&victim).unwrap();
        app.commands().start_render_to(victim.clone());
        assert!(app.workspace().status().contains("refusing to overwrite"), "{}", app.workspace().status());
        let back = image::open(&victim).unwrap().to_rgb8();
        assert_eq!(back.get_pixel(0, 0).0, [10, 20, 30], "the existing photo is untouched");
    }

    /// The mean brightness of a jpg the Save group wrote.
    fn jpg_mean(p: &std::path::Path) -> f64 {
        let img = image::open(p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).to_luma8();
        img.pixels().map(|px| f64::from(px.0[0])).sum::<f64>() / f64::from(img.width() * img.height())
    }

    /// 「Apply this look to every photo in the folder」 over a photo whose
    /// unsaved work waits in the navigation stash: the paste replaces that
    /// work on disk, so the stash entry goes with it — the folder's jpgs, a
    /// reopen and 「Save all」 at exit all read the pasted look, never the old
    /// one (USER_MANUAL: every photo gets the same look and is saved).
    ///
    /// MUTATION THIS CATCHES: drop the stash entries of the photos the paste
    /// wrote (`on_pasted` dropping only the open photo's) and the other
    /// photo's jpg renders its old, stashed look, three stops brighter.
    #[test]
    fn apply_to_every_photo_replaces_unsaved_work_left_on_another_photo() {
        let (mut app, src, _scrub) = normal_photo("applystash");
        let other = src.with_file_name("_gui_normal_applystash_b.jpg");
        let other_dev = autoshade::store::develop_dir(&other);
        let _ = std::fs::remove_dir_all(&other_dev);
        let _scrub_b = Scrub(vec![other_dev]);
        for p in [&src, &other] {
            image::RgbImage::from_pixel(32, 24, image::Rgb([120, 90, 60])).save(p).unwrap();
        }
        // The other photo was edited, much brighter, and left unsaved.
        app.workspace_mut().open_mut().nav_stash.insert(
            other.clone(),
            StashEntry {
                recipe: EditRecipe { exposure_ev: 2.0, ..Default::default() },
                base: None,
                origin: None,
                kind: VariantKind::Original,
                id: ORIGINAL_VARIANT_ID.into(),
                name: None,
                others: Vec::new(),
                active_pos: 0,
            },
        );
        app.editor_mut().doc_mut().recipe.exposure_ev = -1.0;
        let out = std::env::temp_dir().join(format!("autoshade-gui-normal-applystash-out-{}", std::process::id()));
        let _scrub_out = Scrub(vec![out.clone()]);
        app.workspace_mut().exp_mut().dest = ExportDest::Ask;
        app.workspace_mut().normal_mut().folder_pick = Some(Some(out.clone()));
        app.workspace_mut().gal_mut().gallery = vec![src.clone(), other.clone()];
        let ctx = harness::Ctx::new();
        app.commands().start_apply_to_folder();
        land_all(&mut app, &ctx);
        assert!(app.workspace().status().contains('2'), "both photos saved: {}", app.workspace().status());
        assert!(!app.workspace().open().nav_stash.contains_key(&other), "the paste replaced the unsaved work, so reopening reads the paste");
        let disk: EditRecipe =
            serde_json::from_str(&std::fs::read_to_string(autoshade::store::recipe_target(&other)).unwrap()).unwrap();
        assert_eq!(disk.exposure_ev, -1.0, "the look is on disk");
        let jpg = |p: &std::path::Path| {
            let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
            std::fs::read_dir(&out)
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|f| f.file_name().unwrap().to_string_lossy().starts_with(&format!("{stem}.")))
                .unwrap_or_else(|| panic!("no jpg for {stem}"))
        };
        let (open, moved) = (jpg_mean(&jpg(&src)), jpg_mean(&jpg(&other)));
        assert!((open - moved).abs() < 4.0, "the same look on the same pixels: {open} vs {moved}");
    }

    /// Reading the reference folder is work like any other: while it runs
    /// the buttons that start work are locked, and when it lands the look is
    /// made for the photo the folder was picked for — or the line says, in
    /// plain words, that it could not start, instead of dropping it.
    ///
    /// MUTATION THIS CATCHES: leave `busy` off during the build (the save,
    /// the folder delivery and the noise switch start under it), or start the
    /// look on whichever photo is open when the build lands, or drop it
    /// without a word.
    #[test]
    fn reading_the_reference_folder_locks_the_work_and_lands_on_its_own_photo() {
        let (mut app, src, _scrub) = normal_photo("buildbusy");
        let refs = std::env::temp_dir().join(format!("autoshade-gui-normal-buildbusy-refs-{}", std::process::id()));
        std::fs::create_dir_all(&refs).unwrap();
        let _scrub_refs = Scrub(vec![refs.clone()]);
        app.workspace_mut().gal_mut().gallery = vec![src.clone()];
        app.workspace_mut().exp_mut().dest = ExportDest::LastUsed;
        app.workspace_mut().exp_mut().last_dir = Some(refs.clone());
        app.commands().apply_actions(vec![logic::Action::References(refs.clone())]);
        assert!(app.workspace_mut().normal_mut().building);
        assert!(app.workspace().busy(), "the build is work: the buttons that start work wait for it");
        app.commands().start_normal_save();
        assert!(!app.workspace_mut().normal_mut().delivering, "no save starts under the build");
        app.commands().start_apply_to_folder();
        assert!(app.workspace_mut().normal_mut().apply_all.is_none(), "no folder delivery either");
        app.commands().set_normal_denoise(true);
        assert!(app.workspace_mut().normal_mut().denoise_pair.is_none(), "nor the noise switch");
        // The empty folder's build lands (an error) and frees the buttons.
        let ctx = harness::Ctx::new();
        let start = std::time::Instant::now();
        while app.workspace_mut().normal_mut().building {
            assert!(start.elapsed() < std::time::Duration::from_secs(60), "the build never landed");
            std::thread::sleep(std::time::Duration::from_millis(20));
            app.poll_workers(&ctx);
        }
        assert!(!app.workspace().busy(), "the build's landing frees the buttons");
        // A build lands while another photo is open: nothing is made for
        // that photo, and the line says the look did not start.
        app.workspace_mut().normal_mut().building = true;
        app.workspace_mut().normal_mut().build_photo = Some(src.clone());
        app.workspace_mut().set_busy(true);
        app.editor_mut().photo_mut().src_path = Some(src.with_file_name("_gui_normal_buildbusy_other.jpg"));
        app.commands().on_one_click_built(Lang::En, Ok(3));
        assert!(!app.workspace().busy() && !app.workspace_mut().normal_mut().building);
        assert!(app.workspace_mut().normal_mut().last_request.is_none(), "no look for a photo the folder was not picked for");
        let said = tr(Lang::En, K_LOGIC_YOUR_REFERENCE_PHOTOS_ARE_READ_BUT_THE_LOOK_COUL);
        assert_eq!(app.workspace().status(), said);
        assert_eq!(app.workspace_mut().normal_mut().line, crate::panels::normal::NormalLine::Failed(said.to_string()));
        // Back on its own photo, the build hands over to the look.
        app.editor_mut().photo_mut().src_path = Some(src.clone());
        app.workspace_mut().normal_mut().building = true;
        app.workspace_mut().normal_mut().build_photo = Some(src.clone());
        app.workspace_mut().set_busy(true);
        app.commands().on_one_click_built(Lang::En, Ok(3));
        assert!(app.workspace_mut().normal_mut().last_request.is_some(), "the look starts for the photo the folder was picked for");
        land_analysis(&mut app, &ctx);
    }

    /// The noise switch changes the grain, never the look — and never the
    /// look's history: after the ◈ card lands, and after the switch goes off,
    /// Ctrl+Z still steps back through the looks made before (the one-click
    /// line says 「Done — Ctrl+Z goes back」), on the card that is on screen,
    /// its pixels kept.
    ///
    /// MUTATION THIS CATCHES: let the switch's card change reset the undo
    /// history (`load_active`'s `reset_history`) on either path — the
    /// denoise landing or `show_card_keeping_look` — and Ctrl+Z does nothing.
    #[test]
    fn the_noise_switch_keeps_the_looks_undo_history() {
        use crate::panels::normal::DenoisePair;
        let (mut app, src, _scrub) = normal_photo("denoise-undo");
        let ctx = harness::Ctx::new();
        let b0 = std::sync::Arc::new(image::DynamicImage::new_rgba8(4, 4));
        app.editor_mut().doc_mut().variants = vec![Variant {
            id: ORIGINAL_VARIANT_ID.into(),
            name: None,
            kind: VariantKind::Original,
            recipe: EditRecipe::default(),
            base: Some(b0.clone()),
            origin: None,
            thumb: None,
        }];
        app.editor_mut().doc_mut().active = 0;
        app.editor_mut().photo_mut().base_preview = Some(b0.clone());
        { let (e, w, _l) = app.editor_workspace_mut(); e.reset_history(&*w) };
        // A look, then the switch on: its ◈ card lands.
        app.editor_mut().doc_mut().recipe.exposure_ev = 0.5;
        { let (e, w, _l) = app.editor_workspace_mut(); e.commit_now(&*w) };
        app.workspace_mut().normal_mut().denoise_pair =
            Some(DenoisePair { photo: src.clone(), plain: ORIGINAL_VARIANT_ID.into(), denoised: None });
        let master = std::path::PathBuf::from("out/_gui_normal_denoise_undo.denoise.png");
        let note = RetouchNote::Denoised { out: master.clone(), on_mosaic: false };
        let done = Ok((image::DynamicImage::new_rgba8(4, 4), note, master.clone(), RetouchKind::NewDenoised));
        app.services().tx.send(Msg::Retouched(app.workspace().tools().gen_epoch, Box::new(done))).unwrap();
        app.poll_workers(&ctx);
        assert_eq!(app.editor().doc().variants[app.editor().doc().active].kind, VariantKind::Denoised, "the ◈ card is on the canvas");
        let denoised = app.editor().doc().variants[app.editor().doc().active].base.clone().unwrap();
        { let (e, w, l) = app.editor_workspace_mut(); e.undo(w, l, &ctx) };
        assert_eq!(app.editor().doc().recipe.exposure_ev, 0.0, "Ctrl+Z goes back to the look before");
        assert_eq!(app.editor().doc().variants[app.editor().doc().active].kind, VariantKind::Denoised, "on the card on screen");
        assert!(std::sync::Arc::ptr_eq(app.editor().doc().variants[app.editor().doc().active].base.as_ref().unwrap(), &denoised), "its pixels kept");
        { let (e, w, l) = app.editor_workspace_mut(); e.redo(w, l, &ctx) };
        assert_eq!(app.editor().doc().recipe.exposure_ev, 0.5, "and forward again");
        // Another look, then the switch off: back on the plain card, the
        // history still there.
        app.editor_mut().doc_mut().recipe.contrast = 10.0;
        { let (e, w, _l) = app.editor_workspace_mut(); e.commit_now(&*w) };
        app.commands().set_normal_denoise(false);
        assert_eq!(app.editor().doc().variants[app.editor().doc().active].id, ORIGINAL_VARIANT_ID);
        { let (e, w, l) = app.editor_workspace_mut(); e.undo(w, l, &ctx) };
        assert_eq!((app.editor().doc().recipe.exposure_ev, app.editor().doc().recipe.contrast), (0.5, 0.0), "Ctrl+Z after the switch went off");
        assert!(std::sync::Arc::ptr_eq(app.editor().doc().variants[app.editor().doc().active].base.as_ref().unwrap(), &b0), "the plain card's pixels kept");
        { let (e, w, l) = app.editor_workspace_mut(); e.undo(w, l, &ctx) };
        assert_eq!(app.editor().doc().recipe.exposure_ev, 0.0, "all the way back");
    }

    /// 「Save as jpg」 into a folder picked in the save window remembers that
    /// folder, as the workspace's Export does: with the destination 「the
    /// folder you last saved to」 the next save asks nothing.
    ///
    /// MUTATION THIS CATCHES: drop the `last_export_dir` write from
    /// `start_normal_save`'s dialog arm and the next save asks again.
    #[test]
    fn a_normal_save_remembers_the_folder_it_was_saved_to() {
        let (mut app, src, _scrub) = normal_photo("lastdir");
        image::RgbImage::from_pixel(32, 24, image::Rgb([120, 90, 60])).save(&src).unwrap();
        let out = std::env::temp_dir().join(format!("autoshade-gui-normal-lastdir-out-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        let _scrub_out = Scrub(vec![out.clone()]);
        app.workspace_mut().exp_mut().dest = ExportDest::LastUsed;
        app.workspace_mut().exp_mut().last_dir = None;
        assert_eq!(app.workspace().export_dest_dir(), None, "nothing saved yet: the save window asks");
        app.workspace_mut().normal_mut().file_pick = Some(Some(out.join("picked.jpg")));
        app.commands().start_normal_save();
        assert!(app.workspace().busy(), "the save runs: {}", app.workspace().status());
        let ctx = harness::Ctx::new();
        land_all(&mut app, &ctx);
        assert!(out.join("picked.jpg").is_file(), "{}", app.workspace().status());
        assert_eq!(app.workspace().export_dest_dir().as_deref(), Some(out.as_path()), "the next save goes there without asking");
    }
