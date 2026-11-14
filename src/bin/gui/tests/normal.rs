// One part of the GUI's tests (src/bin/gui/tests.rs includes it): Normal's
// four groups in the one workspace's sidebar (panels/normal.rs,
// design-normal-mode.md, design-normal-oneclick.md; 2026-10-23).

    /// One full window — toolbar, status bar, strips, both side panels and
    /// the canvas, as `full_window_shot` lays them — and what it drew: every
    /// text, every vocabulary button of the last frame, and the picture.
    fn window_frame(app: &mut AutoShadeApp, size: egui::Vec2) -> (Vec<String>, Vec<crate::buttons::Drawn>, image::RgbaImage) {
        use crate::buttons::DRAWN;
        let theme = crate::theme::ThemePref::Dark;
        app.theme = theme;
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        crate::theme::install_theme(&ctx, theme);
        let (w, h) = (600usize, 400usize);
        let px: Vec<egui::Color32> = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f32 / w as f32, (i / w) as f32 / h as f32);
                egui::Color32::from_rgb((60.0 + 150.0 * y) as u8, (110.0 + 40.0 * x) as u8, (190.0 - 120.0 * y) as u8)
            })
            .collect();
        app.after_tex = Some(ctx.load_texture("normal-photo", egui::ColorImage::new([w, h], px), egui::TextureOptions::LINEAR));
        let mut tex = ShotTextures::default();
        let mut last = None;
        for i in 0..4 {
            DRAWN.with_borrow_mut(Vec::clear);
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(i as f64 / 2.0),
                ..Default::default()
            };
            input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(1.5);
            let out = ctx.run(input, |ctx| {
                app.upd_top_bar(ctx);
                app.upd_status_bar(ctx);
                app.upd_strips_and_side_panels(ctx);
                app.upd_central(ctx);
            });
            tex.apply(&out.textures_delta);
            last = Some(out);
        }
        let out = last.expect("frames ran");
        let texts = drawn_texts(&out.shapes);
        let drawn = DRAWN.with_borrow(Clone::clone);
        let prims = ctx.tessellate(out.shapes.clone(), out.pixels_per_point);
        (texts, drawn, rasterise(&prims, &tex, size, out.pixels_per_point, egui::Color32::BLACK))
    }

    /// The photo with every button armed, with the key state fixed (no key:
    /// the disclosure line is drawn too). The brush is armed on the real
    /// canvas: the one workspace draws its marks.
    fn app_in(lang: Lang) -> AutoShadeApp {
        let mut app = app_with_every_button(lang);
        app.start_mask_brush(None);
        app.normal.key_present = Some(false);
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
            app.recipe.masks.extend(found_masks());
            let ctx = egui::Context::default();
            crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
            let mut out = None;
            for i in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 20_000.0))),
                    time: Some(i as f64),
                    ..Default::default()
                };
                out = Some(ctx.run(input, |ctx| {
                    ctx.memory_mut(|m| m.set_everything_is_visible(true));
                    controls_panel(ctx, &mut app, 320.0);
                }));
            }
            let texts = drawn_texts(&out.unwrap().shapes);
            let at = |key: &str| texts.iter().position(|t| t.starts_with(key));
            // (a) Portraits, (b) Normal's four groups, (c) the shared panels.
            let order = [
                tr(lang, "People and background"),
                tr(lang, "One-click look"),
                tr(lang, "Fine-tune"),
                tr(lang, "Noise"),
                tr(lang, "Save"),
                tr(lang, "AI generation & reverse-fit"),
                tr(lang, "Tone & WB"),
            ];
            let mut last = None;
            for key in order {
                let here = at(key).unwrap_or_else(|| panic!("{lang:?}: the sidebar draws {key:?}"));
                assert!(last.is_none_or(|l| here > l), "{lang:?}: {key:?} is out of order");
                last = Some(here);
            }
            // Normal's controls are all reachable on that same sidebar.
            for key in [
                tr(lang, "Natural"),
                tr(lang, "Bright & clean"),
                tr(lang, "Warm"),
                tr(lang, "Cool night"),
                tr(lang, "Film"),
                tr(lang, "Black & white"),
                tr(lang, "Match my reference photos…"),
                tr(lang, "Save as jpg"),
                tr(lang, "Apply this look to every photo in the folder"),
            ] {
                assert!(texts.iter().any(|t| t == key), "{lang:?}: {key:?}");
            }
            // …and the full window's toolbar, library rows, canvas marks and
            // variant strip are on, on the same photo.
            let (texts, drawn, img) = window_frame(&mut app, egui::vec2(1400.0, 1000.0));
            let labels: Vec<&str> = drawn.iter().map(|d| d.label.as_str()).collect();
            let has_text = |s: &str| texts.iter().any(|x| x == s);
            let has_button = |s: &str| labels.contains(&s);
            for keep in [
                tr(lang, "Reset"),
                tr(lang, "⭯ Turn left"),
                tr(lang, "⭮ Turn right"),
                tr(lang, "Export"),
                tr(lang, "Save develop"),
                "⌨",
            ] {
                assert!(has_button(keep), "{lang:?}: the toolbar keeps {keep:?}");
            }
            for keep in [tr(lang, "◫ Compare"), tr(lang, "⬛ Single")] {
                assert!(has_text(keep), "{lang:?}: both view switches: {keep:?}");
            }
            for keep in [
                tr(lang, "Copy recipe").to_string(),
                trf(lang, "Paste to selected ({n})", &[("n", "1")]),
                trf(lang, "Render selected ({n})", &[("n", "1")]),
            ] {
                assert!(has_button(&keep), "{lang:?}: the library keeps {keep:?}");
            }
            for keep in [tr(lang, "Fit"), "1:1"] {
                assert!(has_button(keep), "{lang:?}: the canvas keeps {keep:?}");
            }
            assert!(
                texts.iter().any(|x| x.ends_with('%') && x[..x.len() - 1].parse::<f32>().is_ok()),
                "{lang:?}: the zoom readout"
            );
            assert_eq!(app.paint_mode, true, "{lang:?}: the armed brush stays armed");
            assert!(has_text(&app.status), "{lang:?}: the status bar is in the frame's texts");
            assert!(has_text(tr(lang, "▣ Original")), "{lang:?}: the variant strip");
            if let Some(dir) = &shots {
                std::fs::create_dir_all(dir).unwrap();
                img.save(dir.join(format!("window-one-workspace-{lang:?}.png"))).unwrap();
            }
        }
    }

    /// Every text Normal's four groups draw on their own, in one tall frame.
    fn normal_panel_texts(app: &mut AutoShadeApp) -> Vec<String> {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
        let mut out = None;
        for i in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 4000.0))),
                time: Some(i as f64),
                ..Default::default()
            };
            out = Some(ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.normal_panel(ui));
            }));
        }
        drawn_texts(&out.unwrap().shapes)
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
    fn land_analysis(app: &mut AutoShadeApp, ctx: &egui::Context) {
        let start = std::time::Instant::now();
        while app.analyze_inflight {
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
        let mut app = AutoShadeApp { src_path: Some(src.clone()), ..Default::default() };
        app.normal.runner = Some(heuristic_runner);
        (app, src, Scrub(vec![dir, dev]))
    }

    /// A preset card's click builds a DIRECTION request carrying that
    /// preset's text, firm adherence, no library, no reference photos, no
    /// deep thinking, at the look strength — every field the design fixes.
    ///
    /// MUTATION THIS CATCHES: any field of `one_click_request`'s preset arm
    /// changed (the guide, the text, the adherence, the embed switch, the
    /// reference photos, the strength's source) fails an assert below.
    #[test]
    fn a_preset_click_asks_for_that_presets_direction() {
        use crate::buttons::DRAWN;
        let (mut app, _src, _scrub) = normal_photo("preset");
        app.grade_strength = 0.4;
        let ctx = egui::Context::default();
        crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
        let size = egui::vec2(1400.0, 6000.0);
        let mut frame = 0;
        app.normal.key_present = Some(true);
        app_frame(&ctx, &mut app, size, frame, Vec::new());
        frame += 1;
        DRAWN.with_borrow_mut(Vec::clear);
        app_frame(&ctx, &mut app, size, frame, Vec::new());
        frame += 1;
        let mono = DRAWN.with_borrow(|d| d.iter().find(|b| b.label == "Black & white").map(|b| (b.rect, b.cell)));
        let (rect, in_cell) = mono.expect("the Black & white card is drawn");
        assert!(in_cell, "a preset card is a grid cell");
        click_at(&ctx, &mut app, size, &mut frame, rect.center());
        let (req, text, library) = app.normal.last_request.clone().expect("the click started an analysis");
        assert_eq!(req.guide, autoshade::pipeline::Guide::Direction);
        assert_eq!(text.as_deref(), autoshade::advisor::presets::preset_guidance("mono"));
        assert!(text.unwrap().contains("set convert_to_grayscale to true"));
        assert_eq!(req.adherence.get(), crate::panels::normal::PRESET_ADHERENCE);
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
        let ctx = egui::Context::default();
        // What `start_one_click_folder` leaves while the folder is read.
        let building = |app: &mut AutoShadeApp| {
            app.normal.building = true;
            app.normal.build_photo = Some(src.clone());
            app.busy = true;
        };
        building(&mut app);
        app.on_one_click_built(Lang::En, Err("no photos in that folder".into()));
        assert!(app.normal.last_request.is_none(), "a failed build asks nothing");
        assert!(!app.normal.building);
        assert!(matches!(&app.normal.line, crate::panels::normal::NormalLine::Failed(t) if t.contains("no photos")));
        building(&mut app);
        app.on_one_click_built(Lang::En, Ok(3));
        let (req, text, library) = app.normal.last_request.clone().expect("the build handed over to the analysis");
        assert_eq!(req.guide, autoshade::pipeline::Guide::Library);
        assert_eq!(req.style, 1.0);
        assert!(req.style >= autoshade::library::STYLE_TARGET_MIN, "the references are the look to reproduce");
        assert!(req.send_reference_image, "the reference photos go along");
        assert!(req.embed.on(), "the library is searched");
        assert!(!req.think);
        assert_eq!(text, None, "no typed direction");
        assert_eq!(library, autoshade::library::NORMAL_NAME, "Normal's own library");
        assert_eq!(app.normal.line, crate::panels::normal::NormalLine::Working);
        land_analysis(&mut app, &ctx);
    }

    /// With no image key the chain still lands: the built-in proposer's
    /// recipe reaches `on_analyzed`, goes into the sliders, is undoable, and
    /// the one-click line says the built-in look was used.
    #[test]
    fn without_a_key_the_one_click_lands_the_built_in_look() {
        let (mut app, _src, _scrub) = normal_photo("nokey");
        let ctx = egui::Context::default();
        assert_eq!(app.recipe.exposure_ev, 0.0);
        app.start_one_click(crate::panels::normal::OneClick::Preset("natural"));
        assert!(app.busy && app.analyze_inflight);
        land_analysis(&mut app, &ctx);
        assert!(!app.busy);
        assert!(app.recipe.exposure_ev > 0.0, "the dark frame was brightened: {}", app.recipe.exposure_ev);
        assert_eq!(app.normal.line, crate::panels::normal::NormalLine::BuiltIn);
        assert_eq!(app.status, "No AI key set — built-in look used");
        assert_eq!(app.normal.key_present, Some(false));
        let (photo, kept) = app.normal.result.clone().expect("the one-click result is remembered");
        assert_eq!(Some(photo), app.src_path);
        assert_eq!(kept.exposure_ev, app.recipe.exposure_ev);
        // Undoable, like every analysis landing.
        app.commit_now();
        app.undo(&ctx);
        assert_eq!(app.recipe.exposure_ev, 0.0, "Ctrl+Z goes back");
    }

    /// 「Warmth」 is the colour temperature as an offset from the camera's own,
    /// in hundredths of an octave: 0 is the as-shot value, ±100 halve or
    /// double it, and the round trip is exact inside the track.
    #[test]
    fn warmth_is_an_octave_either_side_of_the_as_shot_temperature() {
        use crate::panels::normal::{kelvin_for_warmth, warmth_of};
        let shot = EditRecipe { as_shot_k: Some(5000.0), ..Default::default() };
        assert_eq!(warmth_of(&shot), 0.0, "no custom white balance reads 0");
        assert_eq!(kelvin_for_warmth(&shot, 0.0), 5000.0);
        assert_eq!(kelvin_for_warmth(&shot, 100.0), 10000.0);
        assert_eq!(kelvin_for_warmth(&shot, -100.0), 2500.0);
        for w in [-80.0, -33.0, 0.0, 12.0, 64.0] {
            let r = EditRecipe { temperature_k: Some(kelvin_for_warmth(&shot, w)), ..shot.clone() };
            assert!((warmth_of(&r) - w).abs() < 1e-3, "{w}: {}", warmth_of(&r));
        }
        // A rendered image carries no as-shot value: the develop panel's 5500 K.
        assert_eq!(kelvin_for_warmth(&EditRecipe::default(), 0.0), 5500.0);
        // The track's ends hold.
        let cold = EditRecipe { as_shot_k: Some(3000.0), ..Default::default() };
        assert_eq!(kelvin_for_warmth(&cold, -100.0), 2000.0);
        assert_eq!(warmth_of(&EditRecipe { temperature_k: Some(40000.0), ..shot }), 100.0);
    }

    /// The first-download size the noise switch names is the rendered-image
    /// denoiser's own: its weight plus its network file, as the sidecar pins
    /// them.
    #[test]
    fn the_noise_switch_names_the_downloads_real_size() {
        let py = include_str!("../../../../python/denoise.py");
        let number = |after: &str| -> u64 {
            let at = py.find(after).unwrap_or_else(|| panic!("{after} moved")) + after.len();
            py[at..].chars().skip_while(|c| !c.is_ascii_digit()).take_while(char::is_ascii_digit).collect::<String>().parse().unwrap()
        };
        let weight = {
            let block = &py[py.find("WEIGHT_BYTES = {").expect("WEIGHT_BYTES")..];
            let line = block.lines().find(|l| l.contains("\"color_real_psnr\"")).expect("the default model's size");
            line.chars().filter(char::is_ascii_digit).collect::<String>().parse::<u64>().unwrap()
        };
        let network = number("NETWORK_BYTES =");
        let mb = ((weight + network) as f64 / 1e6).round() as u32;
        assert_eq!(mb, crate::panels::normal::DENOISE_FIRST_DOWNLOAD_MB, "{weight} + {network} bytes");
    }

    /// Poll the app's workers until nothing is running.
    fn land_all(app: &mut AutoShadeApp, ctx: &egui::Context) {
        let start = std::time::Instant::now();
        while app.busy {
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
        let press = |app: &mut AutoShadeApp, ctx: &egui::Context, k: egui::Key| {
            let mut input = egui::RawInput { modifiers: egui::Modifiers::COMMAND, ..Default::default() };
            input.events.push(egui::Event::Key {
                key: k,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            });
            let _ = ctx.run(input, |ctx| app.upd_shortcuts(ctx));
        };
        // Ctrl+E exports into the destination setting's folder.
        let (mut app, src, _scrub) = normal_photo("keys-export");
        image::RgbImage::from_pixel(32, 24, image::Rgb([120, 90, 60])).save(&src).unwrap();
        let out = std::env::temp_dir().join(format!("autoshade-gui-normal-keys-export-out-{}", std::process::id()));
        let _scrub_out = Scrub(vec![out.clone()]);
        app.exp_dest = ExportDest::LastUsed;
        app.last_export_dir = Some(out.clone());
        let ctx = egui::Context::default();
        press(&mut app, &ctx, egui::Key::E);
        assert!(app.busy, "the workspace export started");
        assert!(!app.normal.delivering, "not the Save group's delivery");
        assert!(app.status.contains(&out.file_name().unwrap().to_string_lossy().into_owned()), "{}", app.status);
        land_all(&mut app, &ctx);
        // Ctrl+S saves the develop: the shared ● marker goes clean.
        let (mut app, src, _scrub) = normal_photo("keys-save");
        image::RgbImage::from_pixel(32, 24, image::Rgb([60, 90, 120])).save(&src).unwrap();
        let b0 = std::sync::Arc::new(image::DynamicImage::new_rgba8(4, 4));
        app.variants = vec![Variant {
            id: ORIGINAL_VARIANT_ID.into(),
            name: None,
            kind: VariantKind::Original,
            recipe: EditRecipe::default(),
            base: Some(b0.clone()),
            origin: None,
            thumb: None,
        }];
        app.active = 0;
        app.base_preview = Some(b0);
        app.reset_history();
        app.recipe.exposure_ev = 0.4;
        assert!(app.unsaved_marker_dirty(), "the edit is unsaved before Ctrl+S");
        let ctx = egui::Context::default();
        press(&mut app, &ctx, egui::Key::S);
        assert!(!app.normal.delivering, "not the Save group's delivery");
        assert!(!app.busy);
        assert!(!app.unsaved_marker_dirty(), "the develop was saved: {}", app.status);
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
        let ctx = egui::Context::default();
        app.gallery = vec![src.clone()];
        app.exp_dest = ExportDest::Ask;
        app.status = "Recipe pasted onto 1 photo".into();
        app.normal.folder_pick = Some(None);
        app.start_apply_to_folder();
        assert_eq!(app.status, "", "a cancelled dialog leaves nothing on the line");
        assert!(!app.busy && app.normal.apply_all.is_none(), "and nothing was pasted");
        let out = std::env::temp_dir().join(format!("autoshade-gui-normal-applyall-out-{}", std::process::id()));
        let _scrub_out = Scrub(vec![out.clone()]);
        app.normal.folder_pick = Some(Some(out.clone()));
        app.start_apply_to_folder();
        assert!(app.busy, "the paste runs");
        assert_eq!(app.normal.apply_all.as_deref(), Some(out.as_path()), "into the folder picked first");
        land_all(&mut app, &ctx);
        assert!(app.normal.apply_all.is_none());
    }

    /// The Save group says where the jpg goes, in plain words: never beside
    /// the photo — the app's output folder (Settings), the folder last saved
    /// to, or a question each time.
    #[test]
    fn the_save_line_names_where_the_jpg_goes() {
        for lang in [Lang::En, Lang::Zh] {
            let mut app = app_in(lang);
            app.exp_dest = ExportDest::LastUsed;
            app.last_export_dir = Some(std::env::temp_dir().join("Deliveries"));
            let texts = normal_panel_texts(&mut app);
            let last = trf(lang, "Saves to the folder you last saved to: {folder}", &[("folder", "Deliveries")]);
            assert!(texts.contains(&last), "{lang:?}: {last:?}");
            app.exp_dest = ExportDest::OutFolder;
            let texts = normal_panel_texts(&mut app);
            let root = autoshade::config::delivery_root();
            let name = root.file_name().map_or_else(|| abs_display(&root), |n| n.to_string_lossy().into_owned());
            let out = trf(lang, "Saves to the app's output folder {folder} (change it in Settings ⚙)", &[("folder", &name)]);
            assert!(texts.contains(&out), "{lang:?}: {out:?}");
            app.exp_dest = ExportDest::Ask;
            let texts = normal_panel_texts(&mut app);
            assert!(texts.iter().any(|t| t == tr(lang, "Asks where to save each time")), "{lang:?}: ask");
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
            let plain = tr(lang, "It takes a moment; the preview then shows the result, and that is what gets saved.").to_string();
            let mut app = AutoShadeApp { lang, src_path: Some(PathBuf::from("phone.jpg")), ..Default::default() };
            assert_eq!(app.noise_lines(lang, false), vec![tr(lang, "This download does not include the noise tool.").to_string()]);
            app.normal.denoise_ready = Some(true);
            assert_eq!(app.noise_lines(lang, true), vec![plain.clone()], "{lang:?}: the model is there");
            for ready in [Some(false), None] {
                app.normal.denoise_ready = ready;
                let lines = app.noise_lines(lang, true);
                assert_eq!(lines.len(), 2, "{lang:?} {ready:?}: {lines:?}");
                assert_eq!(lines[0], plain);
                assert!(lines[1].contains("72 MB"), "{lang:?}: {lines:?}");
            }
            app.src_path = Some(PathBuf::from("camera.arw"));
            assert_eq!(app.noise_lines(lang, true), vec![plain.clone()], "{lang:?}: a RAW downloads nothing new");
            let (moment, preview) = match lang {
                Lang::En => ("takes a moment", "preview"),
                Lang::Zh => ("稍等", "预览"),
            };
            assert!(plain.contains(moment) && plain.contains(preview), "{lang:?}: {plain}");
            let mut said = app.noise_lines(lang, true);
            app.src_path = Some(PathBuf::from("phone.jpg"));
            said.extend(app.noise_lines(lang, true));
            said.extend(
                [
                    tr(lang, "Cleans the grain out of the photo, and the preview shows the result — it helps night and dark phone photos most; it uses the graphics card when there is one. Turn it off to see the photo without it."),
                    tr(lang, "Removing the grain… this takes a moment."),
                    tr(lang, "AI denoise done. Turn the switch off to see the photo without it."),
                    tr(lang, "AI denoise on"),
                    tr(lang, "AI denoise off"),
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
        let ctx = egui::Context::default();
        let b0 = std::sync::Arc::new(image::DynamicImage::new_rgba8(4, 4));
        app.variants = vec![Variant {
            id: ORIGINAL_VARIANT_ID.into(),
            name: None,
            kind: VariantKind::Original,
            recipe: EditRecipe::default(),
            base: Some(b0.clone()),
            origin: None,
            thumb: None,
        }];
        app.active = 0;
        app.base_preview = Some(b0);
        app.reset_history();
        // The Retouch door's own gate answers: a turned photo is refused
        // before any worker, with that door's words.
        app.recipe.quarter_turns = 1;
        app.set_normal_denoise(true);
        assert!(!app.busy, "refused before a worker");
        assert!(app.status.contains("this photo is turned"), "{}", app.status);
        assert!(app.normal.denoise_pair.is_none(), "a refused run remembers nothing");
        app.recipe.quarter_turns = 0;
        // The door's worker runs: this photo has no file, so it fails fast at
        // the decode — through `Msg::Retouched`, the Retouch button's message.
        app.set_normal_denoise(true);
        assert!(app.busy, "the denoise worker runs: {}", app.status);
        assert_eq!(app.status, tr(Lang::En, "Removing the grain… this takes a moment."));
        assert!(app.normal_denoise_on(), "the switch reads on while it runs");
        land_all(&mut app, &ctx);
        assert!(!app.normal_denoise_on(), "a failed run leaves the switch off");
        assert_eq!(app.variants.len(), 1, "and no card");
        // A landed run, through the same message the worker sends.
        app.normal.denoise_pair =
            Some(DenoisePair { photo: src.clone(), plain: ORIGINAL_VARIANT_ID.into(), denoised: None });
        let master = std::path::PathBuf::from("out/_gui_normal_denoise.denoise.png");
        let note = RetouchNote::Denoised { out: master.clone(), on_mosaic: false };
        let done = Ok((image::DynamicImage::new_rgba8(4, 4), note, master.clone(), RetouchKind::NewDenoised));
        app.tx.send(Msg::Retouched(app.gen_epoch, Box::new(done))).unwrap();
        app.poll_workers(&ctx);
        assert_eq!(app.variants[app.active].kind, VariantKind::Denoised, "the ◈ card is on the canvas");
        assert_eq!(app.active_source_path().as_deref(), Some(master.as_path()), "…and is what a save renders");
        assert!(app.normal_denoise_on());
        let made = app.variants[app.active].id.clone();
        assert_eq!(app.normal.denoise_pair.as_ref().and_then(|d| d.denoised.clone()), Some(made.clone()), "remembered");
        assert_eq!(app.status, tr(Lang::En, "AI denoise done. Turn the switch off to see the photo without it."));
        // Off: the card it was made from, the look kept.
        app.recipe.exposure_ev = 0.7;
        app.set_normal_denoise(false);
        assert!(!app.busy);
        assert_eq!(app.variants[app.active].id, ORIGINAL_VARIANT_ID, "back to the un-denoised card");
        assert_eq!(app.active_source_path().as_deref(), Some(src.as_path()));
        assert_eq!(app.recipe.exposure_ev, 0.7, "the look rides across");
        assert!(!app.normal_denoise_on());
        assert_eq!(app.status, tr(Lang::En, "AI denoise off"));
        // On again: the same ◈ card, no second run.
        app.recipe.contrast = 12.0;
        app.set_normal_denoise(true);
        assert!(!app.busy, "no second sidecar run");
        assert_eq!(app.variants.len(), 2, "no second card");
        assert_eq!(app.variants[app.active].id, made, "the master already made");
        assert_eq!((app.recipe.exposure_ev, app.recipe.contrast), (0.7, 12.0), "the look rides across");
        assert_eq!(app.status, tr(Lang::En, "AI denoise on"));
        // The switch's way on is the Retouch door, and the save carries no
        // denoise of its own (pinned on the source, the house pattern for a
        // worker whose sidecar cannot run offline).
        let now = include_str!("../panels/normal.rs");
        let on = &now[now.find("pub(crate) fn set_normal_denoise(").expect("set_normal_denoise moved")..];
        let on = &on[..on.find("pub(crate) fn after_normal_denoise(").expect("after_normal_denoise moved")];
        assert!(on.contains("self.start_ai_denoise();"), "the switch no longer runs the Retouch denoise");
        let save = &now[now.find("pub(crate) fn start_normal_save(").expect("start_normal_save moved")..];
        let save = &save[..save.find("pub(crate) fn start_apply_to_folder(").expect("start_apply_to_folder moved")];
        assert!(save.contains("start_render_with(out, export, None, false, how)"), "the save runs a denoise of its own again");
        assert!(!now.contains("default_strength_for"), "the export-time denoise came back");
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
        app.exp_dest = ExportDest::LastUsed;
        app.last_export_dir = Some(out.clone());
        let ctx = egui::Context::default();
        let names = ["_gui_normal_numbered.developed.jpg", "_gui_normal_numbered.developed-2.jpg", "_gui_normal_numbered.developed-3.jpg"];
        let mut sizes = Vec::new();
        for (i, name) in names.iter().enumerate() {
            app.recipe.exposure_ev = 0.2 * (i + 1) as f32;
            app.start_normal_save();
            assert!(app.busy, "save {}: {}", i + 1, app.status);
            assert!(app.status.contains(name), "save {}: the line names the file: {}", i + 1, app.status);
            land_all(&mut app, &ctx);
            let folder = out.file_name().unwrap().to_string_lossy().into_owned();
            assert_eq!(app.status, trf(Lang::En, "Saved {file} to {folder}", &[("file", name), ("folder", &folder)]));
            sizes.push(std::fs::metadata(out.join(name)).map(|m| m.len()).unwrap_or(0));
        }
        assert!(sizes.iter().all(|&n| n > 0), "three real jpgs, none replaced by the next: {sizes:?}");
        // A file picked in the save window: replaced, not numbered.
        let picked = out.join(names[0]);
        let before = std::fs::read(&picked).unwrap();
        app.exp_dest = ExportDest::Ask;
        app.normal.file_pick = Some(Some(picked.clone()));
        app.recipe.exposure_ev = -1.5;
        app.start_normal_save();
        assert!(app.busy, "the picked save runs: {}", app.status);
        land_all(&mut app, &ctx);
        let folder = out.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(app.status, trf(Lang::En, "Saved {file} to {folder}", &[("file", names[0]), ("folder", &folder)]));
        assert_ne!(std::fs::read(&picked).unwrap(), before, "the picked file was replaced");
        let fourth = out.join("_gui_normal_numbered.developed-4.jpg");
        assert!(!fourth.exists(), "a picked file is never numbered");
        // …while the app-made name is numbered again.
        app.exp_dest = ExportDest::LastUsed;
        app.last_export_dir = Some(out.clone());
        app.recipe.exposure_ev = 0.9;
        app.start_normal_save();
        land_all(&mut app, &ctx);
        assert!(fourth.is_file(), "the app-made name steps on to -4: {}", app.status);
        // The whole folder, twice: the second run numbers every photo.
        app.gallery = vec![src.clone(), src2.clone()];
        let all = std::env::temp_dir().join(format!("autoshade-gui-normal-numbered-all-{}", std::process::id()));
        let _scrub_all = Scrub(vec![all.clone()]);
        app.last_export_dir = Some(all.clone());
        for run in 0..2 {
            app.recipe.contrast = 10.0 * (run + 1) as f32;
            app.start_apply_to_folder();
            land_all(&mut app, &ctx);
            let folder = all.file_name().unwrap().to_string_lossy().into_owned();
            let saved = trf(Lang::En, "{n} photos saved to {folder}", &[("n", "2"), ("folder", &folder)]);
            if run == 0 {
                assert_eq!(app.status, saved);
            } else {
                let numbered = trf(Lang::En, "{k} of them have a number added to the name, so no earlier photo was replaced.", &[("k", "2")]);
                assert_eq!(app.status, format!("{saved}. {numbered}"));
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
        app.start_render_to(out.join(names[0]));
        assert!(!app.busy);
        assert!(app.status.contains("refusing to overwrite"), "{}", app.status);
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
        app.exp_dest = ExportDest::LastUsed;
        app.last_export_dir = Some(own.clone());
        let ctx = egui::Context::default();
        app.recipe.exposure_ev = 0.3;
        app.start_normal_save();
        land_all(&mut app, &ctx);
        let landed: Vec<String> = std::fs::read_dir(&own)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".jpg"))
            .collect();
        assert!(
            landed.iter().any(|n| n.starts_with("_gui_normal_ownfolder") && n.ends_with(".jpg")),
            "the jpg landed beside the photo: {landed:?} / {}",
            app.status
        );
        // The photo itself is never replaced: a render aimed at an existing
        // image is refused, and that file keeps its bytes.
        let victim = own.join("_gui_normal_ownfolder_existing.jpg");
        image::RgbImage::from_pixel(32, 24, image::Rgb([10, 20, 30])).save(&victim).unwrap();
        app.start_render_to(victim.clone());
        assert!(app.status.contains("refusing to overwrite"), "{}", app.status);
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
        app.nav_stash.insert(
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
        app.recipe.exposure_ev = -1.0;
        let out = std::env::temp_dir().join(format!("autoshade-gui-normal-applystash-out-{}", std::process::id()));
        let _scrub_out = Scrub(vec![out.clone()]);
        app.exp_dest = ExportDest::Ask;
        app.normal.folder_pick = Some(Some(out.clone()));
        app.gallery = vec![src.clone(), other.clone()];
        let ctx = egui::Context::default();
        app.start_apply_to_folder();
        land_all(&mut app, &ctx);
        assert!(app.status.contains('2'), "both photos saved: {}", app.status);
        assert!(!app.nav_stash.contains_key(&other), "the paste replaced the unsaved work, so reopening reads the paste");
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
        app.gallery = vec![src.clone()];
        app.exp_dest = ExportDest::LastUsed;
        app.last_export_dir = Some(refs.clone());
        app.start_one_click_folder(refs.clone());
        assert!(app.normal.building);
        assert!(app.busy, "the build is work: the buttons that start work wait for it");
        app.start_normal_save();
        assert!(!app.normal.delivering, "no save starts under the build");
        app.start_apply_to_folder();
        assert!(app.normal.apply_all.is_none(), "no folder delivery either");
        app.set_normal_denoise(true);
        assert!(app.normal.denoise_pair.is_none(), "nor the noise switch");
        // The empty folder's build lands (an error) and frees the buttons.
        let ctx = egui::Context::default();
        let start = std::time::Instant::now();
        while app.normal.building {
            assert!(start.elapsed() < std::time::Duration::from_secs(60), "the build never landed");
            std::thread::sleep(std::time::Duration::from_millis(20));
            app.poll_workers(&ctx);
        }
        assert!(!app.busy, "the build's landing frees the buttons");
        // A build lands while another photo is open: nothing is made for
        // that photo, and the line says the look did not start.
        app.normal.building = true;
        app.normal.build_photo = Some(src.clone());
        app.busy = true;
        app.src_path = Some(src.with_file_name("_gui_normal_buildbusy_other.jpg"));
        app.on_one_click_built(Lang::En, Ok(3));
        assert!(!app.busy && !app.normal.building);
        assert!(app.normal.last_request.is_none(), "no look for a photo the folder was not picked for");
        let said = tr(Lang::En, "Your reference photos are read, but the look could not start. Click 「Match my reference photos…」 again.");
        assert_eq!(app.status, said);
        assert_eq!(app.normal.line, crate::panels::normal::NormalLine::Failed(said.to_string()));
        // Back on its own photo, the build hands over to the look.
        app.src_path = Some(src.clone());
        app.normal.building = true;
        app.normal.build_photo = Some(src.clone());
        app.busy = true;
        app.on_one_click_built(Lang::En, Ok(3));
        assert!(app.normal.last_request.is_some(), "the look starts for the photo the folder was picked for");
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
        let ctx = egui::Context::default();
        let b0 = std::sync::Arc::new(image::DynamicImage::new_rgba8(4, 4));
        app.variants = vec![Variant {
            id: ORIGINAL_VARIANT_ID.into(),
            name: None,
            kind: VariantKind::Original,
            recipe: EditRecipe::default(),
            base: Some(b0.clone()),
            origin: None,
            thumb: None,
        }];
        app.active = 0;
        app.base_preview = Some(b0.clone());
        app.reset_history();
        // A look, then the switch on: its ◈ card lands.
        app.recipe.exposure_ev = 0.5;
        app.commit_now();
        app.normal.denoise_pair =
            Some(DenoisePair { photo: src.clone(), plain: ORIGINAL_VARIANT_ID.into(), denoised: None });
        let master = std::path::PathBuf::from("out/_gui_normal_denoise_undo.denoise.png");
        let note = RetouchNote::Denoised { out: master.clone(), on_mosaic: false };
        let done = Ok((image::DynamicImage::new_rgba8(4, 4), note, master.clone(), RetouchKind::NewDenoised));
        app.tx.send(Msg::Retouched(app.gen_epoch, Box::new(done))).unwrap();
        app.poll_workers(&ctx);
        assert_eq!(app.variants[app.active].kind, VariantKind::Denoised, "the ◈ card is on the canvas");
        let denoised = app.variants[app.active].base.clone().unwrap();
        app.undo(&ctx);
        assert_eq!(app.recipe.exposure_ev, 0.0, "Ctrl+Z goes back to the look before");
        assert_eq!(app.variants[app.active].kind, VariantKind::Denoised, "on the card on screen");
        assert!(std::sync::Arc::ptr_eq(app.variants[app.active].base.as_ref().unwrap(), &denoised), "its pixels kept");
        app.redo(&ctx);
        assert_eq!(app.recipe.exposure_ev, 0.5, "and forward again");
        // Another look, then the switch off: back on the plain card, the
        // history still there.
        app.recipe.contrast = 10.0;
        app.commit_now();
        app.set_normal_denoise(false);
        assert_eq!(app.variants[app.active].id, ORIGINAL_VARIANT_ID);
        app.undo(&ctx);
        assert_eq!((app.recipe.exposure_ev, app.recipe.contrast), (0.5, 0.0), "Ctrl+Z after the switch went off");
        assert!(std::sync::Arc::ptr_eq(app.variants[app.active].base.as_ref().unwrap(), &b0), "the plain card's pixels kept");
        app.undo(&ctx);
        assert_eq!(app.recipe.exposure_ev, 0.0, "all the way back");
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
        app.exp_dest = ExportDest::LastUsed;
        app.last_export_dir = None;
        assert_eq!(app.export_dest_dir(), None, "nothing saved yet: the save window asks");
        app.normal.file_pick = Some(Some(out.join("picked.jpg")));
        app.start_normal_save();
        assert!(app.busy, "the save runs: {}", app.status);
        let ctx = egui::Context::default();
        land_all(&mut app, &ctx);
        assert!(out.join("picked.jpg").is_file(), "{}", app.status);
        assert_eq!(app.export_dest_dir().as_deref(), Some(out.as_path()), "the next save goes there without asking");
    }
