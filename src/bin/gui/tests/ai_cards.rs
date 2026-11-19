// One part of the GUI's tests (src/bin/gui/tests.rs includes it): the pristine AI card and what lands beside it: edits, versions, fills, adjusts, paints, retouches, reverse fits, denoises and stacks.

    /// The immutability rule (2026-09-13, user decision): an edit made while
    /// the pristine ✨ card is active continues on a new ✎ card on the same
    /// raster; the ✨ card goes back to neutral, keeps its identity and its
    /// name, and the canvas is untouched. The frame hook that applies it
    /// sits between the panels and the develop dispatch.
    #[test]
    fn an_edit_on_the_pristine_ai_card_continues_on_a_new_edited_card() {
        let (mut app, _ctx, _dir, _src, master, ai_px, _scrub) =
            ai_card_fixture("fork", EditRecipe::default());
        assert_eq!(strip_kinds(&app), vec![VariantKind::Original, VariantKind::Generated]);
        assert_eq!(app.editor().doc().active, 1);
        assert!(app.editor().doc().recipe.is_noop(), "premise: the ✨ card is pristine");
        assert!(!app.editor().unsaved_marker_dirty(), "premise: a clean open — {}", app.workspace().status());
        assert!(!app.commands().fork_edited_card(), "a neutral recipe on the ✨ card forks nothing");
        assert_eq!(app.editor().doc().variants.len(), 2);

        app.editor_mut().doc_mut().recipe.contrast = 7.0;
        app.editor_mut().session_mut().dirty = true;
        assert!(app.commands().fork_edited_card(), "the first edit forks");
        assert_eq!(
            strip_kinds(&app),
            vec![VariantKind::Original, VariantKind::Generated, VariantKind::Edited]
        );
        assert_eq!(app.editor().doc().active, 2);
        assert_eq!(app.editor().doc().variants[2].recipe.contrast, 7.0);
        assert!(app.editor().doc().variants[1].recipe.is_noop(), "the ✨ card is pristine again");
        assert_eq!(app.editor().doc().variants[1].id, "gen-1", "…and keeps its identity");
        assert_eq!(app.editor().doc().variants[1].name.as_deref(), Some("sky"), "…and its name");
        assert!(
            !app.editor().doc().variants[2].id.is_empty() && app.editor().doc().variants[2].id != "gen-1",
            "the ✎ card is born with its own identity"
        );
        assert_eq!(app.editor().doc().variants[2].name, None);
        assert!(
            app.editor().doc().variants[2].base.as_ref().is_some_and(|b| Arc::ptr_eq(b, &ai_px)),
            "the same pixels"
        );
        assert_eq!(app.editor().doc().variants[2].origin.as_deref(), Some(master.as_path()));
        assert_eq!(app.editor().doc().recipe.contrast, 7.0, "the canvas is untouched");
        assert!(app.editor().session().dirty, "…and its pending develop still runs");
        assert!(!app.commands().fork_edited_card(), "an ✎ card never forks");
        assert_eq!(app.editor().doc().variants.len(), 3);
        assert!(app.editor().unsaved_marker_dirty(), "a new card is unsaved work");
        assert!(app.workspace().chrome().toasts.iter().any(|t| t.text.contains("✎")), "said by toast");

    }

    /// A version loaded onto the pristine ✨ card is an edit like any other.
    #[test]
    fn a_version_loaded_onto_the_pristine_ai_card_lands_on_an_edited_card() {
        let (mut app, _ctx, _dir, src, _master, _ai_px, _scrub) =
            ai_card_fixture("version", EditRecipe::default());
        std::fs::write(
            autoshade::store::version_target(&src, 1),
            serde_json::to_string(&EditRecipe {
                contrast: 7.0,
                base_curve: vec![[0.0, 0.0], [0.5, 0.62], [1.0, 1.0]],
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        app.commands().load_version(1);
        assert_eq!(
            strip_kinds(&app),
            vec![VariantKind::Original, VariantKind::Generated, VariantKind::Edited],
            "{}",
            app.workspace().status()
        );
        assert_eq!(app.editor().doc().active, 2);
        assert_eq!(app.editor().doc().recipe.contrast, 7.0);
        assert!(app.editor().doc().recipe.base_curve.is_empty(), "calibration stripped onto AI pixels");
        assert!(app.editor().doc().variants[1].recipe.is_noop(), "the ✨ card stays pristine");
        assert!(app.workspace().status().starts_with("Loaded version v1"), "{}", app.workspace().status());
    }

    /// The Analyze landing forks BEFORE it persists, so the record it writes
    /// already names the ✎ card — and the pristine card is saved pristine.
    #[test]
    fn an_analyze_landing_on_the_pristine_ai_card_forks_before_it_saves() {
        let (mut app, _ctx, _dir, src, master, _ai_px, _scrub) =
            ai_card_fixture("analyze", EditRecipe::default());
        let epoch = app.workspace().tools().gen_epoch;
        app.commands().on_analyzed(
            Lang::En,
            epoch,
            Box::new(Ok((
                EditRecipe {
                    contrast: 7.0,
                    base_curve: vec![[0.0, 0.0], [0.5, 0.62], [1.0, 1.0]],
                    ..Default::default()
                },
                autoshade::advisor::Verdict {
                    decision: autoshade::advisor::Decision::Accept,
                    reasons: Vec::new(),
                    revised_hint: None,
                },
                Vec::new(),
            ))),
        );
        assert_eq!(
            strip_kinds(&app),
            vec![VariantKind::Original, VariantKind::Generated, VariantKind::Edited],
            "{}",
            app.workspace().status()
        );
        assert_eq!(app.editor().doc().active, 2);
        assert_eq!(app.editor().doc().recipe.contrast, 7.0);
        assert!(app.editor().doc().recipe.base_curve.is_empty());
        assert!(app.editor().doc().variants[1].recipe.is_noop());
        let rec = saved_strip_of(&src);
        assert_eq!(rec.active_kind, "edited");
        assert_eq!(rec.active_pos, 2);
        assert_eq!(
            rec.others.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(),
            vec!["original", "generated"]
        );
        assert!(rec.others[1].recipe.is_noop(), "the pristine card is saved pristine");
        assert_eq!(rec.others[1].id.as_deref(), Some("gen-1"));
        let disk: EditRecipe = serde_json::from_str(
            &std::fs::read_to_string(autoshade::store::recipe_target(&src)).unwrap(),
        )
        .unwrap();
        assert_eq!(disk.contrast, 7.0);
        assert!(!disk.base_curve.is_empty(), "the disk form keeps the calibration");
        let (origin, generated) = autoshade::store::read_pixel_source(&src).expect("pixels.json");
        assert!(generated);
        assert_eq!(origin, master);
        assert!(!autoshade::pipeline::xmp_target(&src).exists(), "no projection over AI pixels");
        assert!(!app.editor().unsaved_marker_dirty(), "the landing saved the forked strip: {}", app.workspace().status());
    }

    /// 2026-10-01 (user decision, replacing 2026-09-15's new-card landing): a
    /// generative fill lands as a LAYER on the card it was made on — the
    /// strip does not grow, the card keeps its develop, base and pixel
    /// source, and its recipe gains one drawing `PixelLayer` naming the
    /// artifact by its absolute path. One Ctrl+Z takes the layer off. On a
    /// pristine ✨ card the layer is that card's first edit, so it continues
    /// on an ✎ card and the ✨ card stays as generated.
    #[test]
    fn a_fill_lands_as_a_layer_on_the_card_it_was_made_on() {
        let ctx = egui::Context::default();
        let src = std::path::PathBuf::from("_fill_layer_test.ARW");
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {
     src_path: Some(src.clone()),
     ..Default::default()
 }; editor });
        let b0 = std::sync::Arc::new(image::DynamicImage::new_rgba8(4, 4));
        let developed = EditRecipe { contrast: 7.0, ..Default::default() };
        let card = |kind, id: &str, recipe: &EditRecipe, origin: Option<std::path::PathBuf>| Variant {
            id: id.into(),
            name: None,
            kind,
            recipe: recipe.clone(),
            base: Some(b0.clone()),
            origin,
            thumb: None,
        };
        app.editor_mut().doc_mut().variants = vec![card(VariantKind::Original, ORIGINAL_VARIANT_ID, &developed, None)];
        app.editor_mut().doc_mut().active = 0;
        app.editor_mut().doc_mut().recipe = developed.clone();
        app.editor_mut().photo_mut().base_preview = Some(b0.clone());
        { let (e, w, _l) = app.editor_workspace_mut(); e.reset_history(&*w) };
        let out = std::path::PathBuf::from("out/_fill_layer_test.fill.png");
        let land = |app: &mut AutoShadeApp| {
            let epoch = app.workspace().tools().gen_epoch;
            app.commands().on_retouched(
                &ctx,
                Lang::En,
                epoch,
                Ok((image::DynamicImage::new_rgba8(4, 4), RetouchNote::Filled(out.clone()), out.clone(), RetouchKind::Layer)),
            );
        };
        land(&mut app);
        assert_eq!(strip_kinds(&app), vec![VariantKind::Original], "{}", app.workspace().status());
        assert_eq!(app.editor().doc().active, 0, "the canvas stays on the card it filled");
        let layers = &app.editor().doc().recipe.pixel_layers;
        assert_eq!(layers.len(), 1, "one layer on the card");
        assert!(layers[0].draws(), "a new layer is on, at full opacity");
        let abs = std::path::absolute(&out).unwrap();
        assert_eq!(layers[0].path, abs.display().to_string(), "the layer names its file absolutely");
        assert_eq!(app.editor().doc().recipe.contrast, 7.0, "the card keeps its develop under the layer");
        assert!(
            app.editor().doc().variants[0].base.as_ref().is_some_and(|b| std::sync::Arc::ptr_eq(b, &b0)),
            "…and its base"
        );
        assert_eq!(app.editor().doc().variants[0].origin, None, "…and its pixel source");
        assert!(app.workspace().status().contains("layer"), "{}", app.workspace().status());
        { let (e, w, l) = app.editor_workspace_mut(); e.undo(w, l, &ctx) };
        assert!(app.editor().doc().recipe.pixel_layers.is_empty(), "one Ctrl+Z takes the layer off");
        assert_eq!(app.editor().doc().recipe.contrast, 7.0, "…and only the layer");

        // On a pristine ✨ card: the layer continues on an ✎ card.
        let generated = EditRecipe::default();
        app.editor_mut().doc_mut().variants = vec![card(VariantKind::Generated, "gen", &generated, Some("out/_fill_layer_test.reimagine.png".into()))];
        app.editor_mut().doc_mut().active = 0;
        app.editor_mut().doc_mut().recipe = generated.clone();
        { let (e, w, _l) = app.editor_workspace_mut(); e.reset_history(&*w) };
        land(&mut app);
        assert_eq!(strip_kinds(&app), vec![VariantKind::Generated, VariantKind::Edited]);
        assert_eq!(app.editor().doc().active, 1, "the ✎ card is under the canvas");
        assert!(app.editor().doc().variants[0].recipe.pixel_layers.is_empty(), "the ✨ card stays as generated");
        assert_eq!(app.editor().doc().recipe.pixel_layers.len(), 1, "the layer is the ✎ card's");

    }

    /// Both worker arms land through the same new-card path; the source
    /// raster/recipe survive, and the new card can be adjusted again.
    #[test]
    fn an_adjust_lands_as_a_new_generated_card_and_leaves_the_source_card_alone() {
        let dir = std::env::temp_dir().join(format!("autoshade-gui-adjust-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("library")).unwrap();
        for lang in [Lang::En, Lang::Zh] {
            for region in [false, true] {
                let ctx = egui::Context::default();
                let src = dir.join("library").join(format!("adjust-{lang:?}-{region}.png"));
                let base = Arc::new(image::DynamicImage::new_rgb8(4, 4));
                let origin = dir.join(format!("generated-{lang:?}-{region}.png"));
                let developed = EditRecipe {
                    contrast: 7.0, straighten_deg: 2.0,
                    crop: Some(autoshade::recipe::Crop { top: 0.1, left: 0.1, bottom: 0.9, right: 0.9 }),
                    ..Default::default()
                };
                let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {

                        src_path: Some(src.clone()),

                        base_preview: Some(base.clone()),

                        ..Default::default()

                    }; *editor.doc_mut() = Document {


                        variants: vec![Variant {
                        id: new_variant_id(), name: None, kind: VariantKind::Generated,
                        recipe: EditRecipe::default(), base: Some(base.clone()),
                        origin: Some(origin.clone()), thumb: None,
                    }],


                        ..Default::default()


                    }; editor });
                { let (e, w, _l) = app.editor_workspace_mut(); e.reset_history(&*w) };
                let out = unique_out(&src, "adjust").unwrap();
                assert_eq!(out.file_name().unwrap().to_string_lossy(),
                    format!("adjust-{lang:?}-{region}.adjust.png"));
                let second = unique_out(&src, "adjust").unwrap();
                assert_eq!(second.file_name().unwrap().to_string_lossy(),
                    format!("adjust-{lang:?}-{region}.adjust-2.png"));
                release_empty_claim(&second);
                let source_recipe = app.editor().doc().variants[0].recipe.clone();
                let epoch = app.workspace().tools().gen_epoch;
                app.commands().on_retouched(&ctx, lang, epoch, Ok((
                    image::DynamicImage::new_rgb8(4, 4),
                    RetouchNote::Adjusted { out: out.clone(), region, divergence: (!region).then_some(0.12) },
                    out.clone(), RetouchKind::NewGenerated,
                )));
                assert_eq!(strip_kinds(&app), vec![VariantKind::Generated, VariantKind::Generated]);
                assert_eq!(app.editor().doc().active, 1, "the new adjusted card is active");
                assert_eq!(app.editor().doc().variants[0].origin.as_ref(), Some(&origin), "the source keeps its pixels");
                assert_eq!(app.editor().doc().variants[0].recipe, source_recipe, "the source keeps its recipe");
                assert!(Arc::ptr_eq(app.editor().doc().variants[0].base.as_ref().unwrap(), &base));
                assert_eq!(app.editor().doc().variants[1].origin.as_ref(), Some(&out));
                assert_eq!(app.editor().active_source_path().as_ref(), Some(&out), "the next adjust follows this master");
                assert!(app.editor().doc().recipe.is_noop() && app.editor().doc().variants[1].recipe.is_noop());
                assert_eq!(app.editor().fit_target(&app.workspace()).as_ref(), Some(&out), "reverse-fit reads the new generated card");
                let note = match (lang, region) {
                    (Lang::En, true) => "adjusted → new ✨ card (painted area only)",
                    (Lang::En, false) => "adjusted → new ✨ card (whole image, divergence D = 0.12)",
                    (Lang::Zh, true) => "已调整 → 新 ✨ 卡（只改涂抹区域）",
                    (Lang::Zh, false) => "已调整 → 新 ✨ 卡（整张图，结构偏离 D = 0.12）",
                };
                assert!(app.workspace().status().starts_with(note), "landing language: {}", app.workspace().status());
                // An edited AI card's non-neutral develop is just as immutable
                // under another adjust; crop/straighten do not reach the result.
                app.editor_mut().doc_mut().variants[1].kind = VariantKind::Edited;
                app.editor_mut().doc_mut().variants[1].recipe = developed.clone();
                app.editor_mut().doc_mut().recipe = developed.clone();
                let epoch = app.workspace().tools().gen_epoch;
                app.commands().on_retouched(&ctx, lang, epoch, Ok((
                    image::DynamicImage::new_rgb8(4, 4),
                    RetouchNote::Adjusted { out: second.clone(), region, divergence: (!region).then_some(0.0) },
                    second.clone(), RetouchKind::NewGenerated,
                )));
                assert_eq!(app.editor().doc().active, 2);
                assert_eq!(app.editor().doc().variants[1].recipe, developed);
                assert_eq!(app.editor().doc().variants[1].origin.as_ref(), Some(&out));
                assert!(app.editor().doc().variants[2].recipe.is_noop());
                release_empty_claim(&out);
            }
        }        let _ = std::fs::remove_dir_all(&dir);
    }

    fn draw_adjust_panel(app: &mut AutoShadeApp) -> Vec<String> {
        let ctx = egui::Context::default();
        crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
        let mut texts = Vec::new();
        for _ in 0..3 {
            texts = draw_adjust_frame(app, &ctx);
        }
        texts
    }

    fn draw_adjust_frame(app: &mut AutoShadeApp, ctx: &egui::Context) -> Vec<String> {
        let out = ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 20_000.0))),
            ..Default::default()
        }, |ctx| {
            ctx.memory_mut(|m| m.set_everything_is_visible(true));
            egui::SidePanel::left("adjust-controls").default_width(320.0).show(ctx, |ui| app.commands().ai_panel(ui));
        });
        drawn_texts(&out.shapes)
    }

    #[test]
    fn the_adjust_verb_is_disabled_off_ai_pixels_and_with_no_prompt_and_no_strokes() {
        // The fold's own area is the live canvas here.
        let mut app = AutoShadeApp::default().but_workspace({ let mut view = WorkspaceView::default(); *view.masks_mut() = MaskEditState {
                paint_owner: BrushOwner::Adjust,
                ..Default::default()
            }; view });
        // Off AI pixels, empty AI input, and useful AI input are the three
        // states. Both Generated and Edited count; busy still disables them.
        for kind in [VariantKind::Original, VariantKind::Fitted, VariantKind::Denoised,
            VariantKind::Stacked, VariantKind::Generated, VariantKind::Edited]
        {
            app.editor_mut().doc_mut().variants = vec![Variant {
                id: new_variant_id(), name: None, kind, recipe: EditRecipe::default(),
                base: None, origin: None, thumb: None,
            }];
            for (prompt, strokes, busy) in [("bluer sky", false, false), (" \t ", false, false),
                ("", true, false), ("bluer sky", true, true)]
            {
                app.workspace_mut().genai_mut().adjust_prompt = prompt.into();
                app.workspace_mut().set_busy(busy);
                app.workspace_mut().masks_mut().mask_paint = strokes.then(|| image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 64, 64, 160])));
                app.workspace_mut().paint_mask_changed(None);
                draw_adjust_panel(&mut app);
                let want = !busy && kind.on_ai_pixels() && (strokes || !prompt.trim().is_empty());
                assert_eq!(app.seams().adjust_btn_enabled, Some(want),
                    "the adjust verb must stay disabled off AI pixels or without prompt/strokes (kind={kind:?}, prompt={prompt:?}, strokes={strokes}, busy={busy})");
            }
        }
        // The starter uses the same rule before claiming output or spawning.
        app.workspace_mut().set_busy(false);
        app.workspace_mut().genai_mut().adjust_prompt.clear();
        app.workspace_mut().masks_mut().mask_paint = None;
        app.workspace_mut().paint_mask_changed(Some(false));
        app.editor_mut().photo_mut().src_path = Some(PathBuf::from("adjust-guard.png"));
        app.commands().start_adjust();
        assert!(!app.workspace().busy(), "blank whole-image adjustment never reaches a worker");
    }

    #[test]
    fn the_adjust_fold_says_whether_it_reads_the_painted_area() {
        for lang in [Lang::En, Lang::Zh] {
            let mut app = AutoShadeApp::default().but_lang(lang);
            app.editor_mut().doc_mut().variants = vec![Variant {
                id: new_variant_id(), name: None, kind: VariantKind::Generated,
                recipe: EditRecipe::default(), base: None, origin: None, thumb: None,
            }];
            app.workspace_mut().masks_mut().paint_owner = BrushOwner::Adjust; // the fold's own area is the live canvas here
            // The same >10 alpha predicate as export: a faint/erased mask
            // is whole-image mode, even though the mask buffer exists.
            for alpha in [0, 10, 11, 160] {
                app.workspace_mut().masks_mut().mask_paint = Some(image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 64, 64, alpha])));
                app.workspace_mut().paint_mask_changed(None);
                let strokes = alpha > 10;
                assert_eq!(app.workspace().has_painted_mask(), strokes);
                assert_eq!(app.workspace().export_mask_png().is_some(), strokes);
                let text = draw_adjust_panel(&mut app);
                let painted = tr(lang, K_AI_PAINTED_AREA_ONLY);
                let whole = tr(lang, K_AI_WHOLE_IMAGE_PAINT_AN_AREA_TO_LIMIT_IT);
                let (yes, no) = if strokes { (painted, whole) } else { (whole, painted) };
                assert!(text.iter().any(|t| t == yes), "status missing: {yes}");
                assert!(!text.iter().any(|t| t == no), "stale status: {no}");
            }
            app.editor_mut().doc_mut().variants[0].kind = VariantKind::Original;
            let text = draw_adjust_panel(&mut app);
            assert!(text.iter().any(|t| t == tr(lang, K_AI_SELECT_A_AI_GENERATED_CARD_OR_ITS_EDIT_FIRST)));
            assert!(!text.iter().any(|t| t == tr(lang, K_AI_PAINTED_AREA_ONLY)));
        }
        let prefs = Prefs { adjust_quality: 2, ..Prefs::default() };
        let json = serde_json::to_string(&prefs).unwrap();
        let decoded: Prefs = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.adjust_quality, 2);
        let older = json.replace(r#""adjust_quality":2,"#, "");
        assert!(!older.contains("adjust_quality"));
        let old: Prefs = serde_json::from_str(&older).unwrap();
        assert_eq!(old.adjust_quality, 0, "old prefs keep high as the default");
        assert!(!json.contains("adjust_prompt"), "prompt is transient like Reimagine's");
        // The value crosses the prefs file both ways: restore clamps it to the
        // panel's three choices, save writes the app's value back.
        let mut restored = AutoShadeApp::default();
        restored.restore_prefs(Prefs { adjust_quality: 7, ..Prefs::default() });
        assert_eq!(restored.workspace().genai().adjust_quality, 2, "restore clamps to the panel's choices");
        let mut app = AutoShadeApp::default().but_workspace({ let mut view = WorkspaceView::default(); *view.genai_mut() = GenerativeState { adjust_quality: 1, ..Default::default() }; view });
        let storage = harness::PrefsStore::save(&mut app);
        let saved = storage.prefs().unwrap();
        assert_eq!(saved.adjust_quality, 1, "save writes the app's value back");
    }

    /// Real pointer events cover both stamp branches. The counter measures
    /// buffer walks, not elapsed time, so a small fast machine cannot hide a
    /// per-frame scan of an empty preview (the normal state of the fold).
    #[test]
    fn the_adjust_fold_scans_only_after_a_paint_mask_change() {
        fn stroke(app: &mut AutoShadeApp, drag: bool) {
            let ctx = egui::Context::default();
            let frame = |app: &mut AutoShadeApp, events: Vec<egui::Event>| {
                let mut painted_rect = egui::Rect::NOTHING;
                let _ = ctx.run(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 850.0))),
                    events, ..Default::default()
                }, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(1024.0, 768.0), egui::Sense::click_and_drag());
                        painted_rect = rect;
                        app.commands().handle_paint(&resp, ViewXform {
                            rect, uv: egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        });
                    });
                });
                painted_rect
            };
            let rect = frame(app, vec![]);
            let start = rect.center();
            let end = start + egui::vec2(if drag { 48.0 } else { 0.0 }, 0.0);
            let button = |pos, pressed| egui::Event::PointerButton {
                pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE,
            };
            frame(app, vec![egui::Event::PointerMoved(start), button(start, true)]);
            if drag {
                frame(app, vec![egui::Event::PointerMoved(end)]);
            }
            frame(app, vec![button(end, false)]);
        }
        fn five_frames(app: &mut AutoShadeApp, ctx: &egui::Context, painted: bool, why: &str) {
            let mut text = Vec::new();
            for _ in 0..5 {
                // A texture upload consumes this flag; it must not consume
                // or invalidate the independent paint-presence memo.
                app.workspace_mut().overlay_mut().mask_dirty = false;
                text = draw_adjust_frame(app, ctx);
            }
            let region = "painted area only";
            let whole = "whole image (paint an area to limit it)";
            let (yes, no) = if painted { (region, whole) } else { (whole, region) };
            assert!(text.iter().any(|t| t == yes), "{why}: expected {yes}");
            assert!(!text.iter().any(|t| t == no), "{why}: stale {no}");
            assert_eq!(app.workspace().has_painted_mask(), painted, "{why}");
        }
        for drag in [false, true] {
            let base = Arc::new(image::DynamicImage::new_rgb8(1024, 768));
            let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {

                    base_preview: Some(base.clone()),

                    ..Default::default()

                }; *editor.doc_mut() = Document {


                    variants: (0..2).map(|_| Variant {
                    id: new_variant_id(), name: None, kind: VariantKind::Generated,
                    recipe: EditRecipe::default(), base: Some(base.clone()), origin: None, thumb: None,
                }).collect(),


                    ..Default::default()


                }; editor });
            // The fold's OWN brush on a canvas sized to the plate: the Adjust
            // area is the live canvas (2026-09-27, `BrushOwner`).
            let __lang = app.lang();
            app.workspace_mut().rebind_paint_canvas(__lang, 1024, 768);
            app.commands().arm_brush(BrushOwner::Adjust);
            let ctx = egui::Context::default();
            crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
            five_frames(&mut app, &ctx, false, "an empty brush reads the whole image");
            assert!(app.workspace().overlay().mask_presence_scans.get() <= 1, "five empty frames may scan only once");
            let empty_scans = app.workspace().overlay().mask_presence_scans.get();

            stroke(&mut app, drag);
            assert!(app.workspace().masks().mask_paint.as_ref().unwrap().pixels().any(|p| p[3] > 10), "the real brush added paint");
            five_frames(&mut app, &ctx, true, "a brush stroke limits the adjust to the painted area");
            assert_eq!(app.workspace().overlay().mask_presence_scans.get(), empty_scans, "adding paint needs no presence scan");
            assert!(app.workspace().export_area_png(BrushOwner::Adjust).is_some());

            // 「Clear area」 is the fold's eraser (the retouch brushes have no
            // erase mode): the canvas is known blank, so nothing scans.
            app.workspace_mut().clear_area(BrushOwner::Adjust);
            assert!(app.workspace().masks().mask_paint.as_ref().unwrap().pixels().all(|p| p[3] <= 10), "clearing removed every stroke");
            five_frames(&mut app, &ctx, false, "clearing the area must return the adjust fold to whole image");
            assert_eq!(app.workspace().overlay().mask_presence_scans.get(), empty_scans, "a cleared area needs no scan");
            assert!(app.workspace().export_area_png(BrushOwner::Adjust).is_none());

            stroke(&mut app, drag);
            assert!(app.workspace().has_painted_mask());
            let before_switch = app.workspace().overlay().mask_presence_scans.get();
            app.commands().switch_variant(1, &ctx);
            assert_eq!(app.editor().doc().active, 1);
            five_frames(&mut app, &ctx, false, "a fresh card starts with an empty brush");
            assert_eq!(app.workspace().overlay().mask_presence_scans.get(), before_switch, "a fresh card needs zero scans");

            // The other plate-replacement door is also known empty.
            let __lang = app.lang();
            app.workspace_mut().rebind_paint_canvas(__lang, 1024, 768);
            five_frames(&mut app, &ctx, false, "a rebound canvas starts empty");
            assert_eq!(app.workspace().overlay().mask_presence_scans.get(), before_switch, "rebinding needs zero scans");
        }
    }

    /// Census the entire GUI tree, including the raw mutable borrows. A new
    /// replacement/write site must declare its notification here; the sole
    /// texture-dirty setter is itself the presence-invalidation owner.
    #[test]
    fn every_paint_buffer_mutation_uses_the_presence_notification_door() {
        fn walk_rs(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("GUI source dir listable") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    walk_rs(&path, out);
                } else if path.extension().is_some_and(|x| x == "rs") {
                    out.push(path);
                }
            }
        }
        const CENSUS: [(&str, usize); 9] = [
            ("variant_actions.rs::load_active", 1),
            ("workspace.rs::rebind_paint_canvas", 1),
            ("editor.rs::paint_imported_removals", 1),
            ("workspace.rs::paint_mask_changed", 1),
            ("workspace.rs::clear_mask", 1),
            ("workspace.rs::select_brush_owner", 1),
            ("masks.rs::start_mask_brush", 1),
            ("workspace.rs::unstash_owner_area", 1),
            ("panels/retouch.rs::handle_paint", 1),
        ];
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/bin/gui");
        let mut sources = Vec::new();
        walk_rs(&root, &mut sources);
        assert!(sources.len() >= 6, "expected the split GUI module tree");
        let replace = concat!("self.", "workspace.masks_mut().", "mask_paint = Some(");
        let dirty = concat!("self.", "workspace.overlay_mut().", "mask_dirty = true");
        let dirty_in_ws = concat!("self.", "overlay_mut().", "mask_dirty = true");
        let replace_in_ws = concat!("self.", "masks_mut().", "mask_paint = Some(");
        let borrow_in_ws = concat!("&mut self.", "masks_mut().", "mask_paint");
        let mutations = [replace, replace_in_ws, borrow_in_ws, dirty, dirty_in_ws, concat!("masks.mask_paint", ".as_mut()"), concat!("&mut self.", "workspace.masks_mut().", "mask_paint")];
        let mut found: std::collections::BTreeMap<String, usize> = Default::default();
        for path in sources {
            let text = std::fs::read_to_string(&path).expect("GUI source readable");
            let name = path.strip_prefix(&root).unwrap().to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
            let lines: Vec<_> = text.lines().collect();
            let mut function = "";
            let mut start = 0;
            for (i, line) in lines.iter().enumerate() {
                if let Some(head) = line.strip_prefix("    pub(crate) fn ").or_else(|| line.strip_prefix("    fn ")) {
                    function = head.split('(').next().unwrap();
                    start = i;
                }
                if line.trim_start().starts_with("//") || !mutations.iter().any(|m| line.contains(m)) {
                    continue;
                }
                let site = format!("{name}::{function}");
                *found.entry(site.clone()).or_default() += 1;
                if line.contains(dirty) || line.contains(dirty_in_ws) {
                    assert_eq!(site, "workspace.rs::paint_mask_changed", "only the notification door may mark the overlay dirty");
                    continue;
                }
                if site.starts_with("workspace.rs::") || site.starts_with("editor.rs::") {
                    // The workspace owns these two doors now; their own bodies
                    // are the notification path (the masks.rs sites below still
                    // pin every caller).
                    continue;
                }
                let end = lines[start..].iter().position(|l| *l == "    }").expect("method end") + start;
                let body = lines[start..=end].join("\n");
                assert!(body.contains("self.workspace.paint_mask_changed("), "{site} bypasses paint-presence invalidation");
                if line.contains(replace) {
                    assert!(lines[i + 1].trim_start().starts_with("self.workspace.paint_mask_changed("),
                        "{site}: a replacement must immediately notify paint presence");
                }
            }
        }
        let expected = CENSUS.into_iter().map(|(site, n)| (site.to_string(), n)).collect();
        assert_eq!(found, expected, "paint-buffer mutation sites changed: wire the notification door and update the census");
        assert_eq!(found.values().sum::<usize>(), 9, "the moved pairs count once each");
    }

    /// The immutability rule for PIXELS: an in-place retouch (heal / clone /
    /// denoise) on the pristine ✨ card bakes into a new ✎ card; the ✨ card
    /// keeps its raster.
    #[test]
    fn a_retouch_in_place_on_the_pristine_ai_card_bakes_into_an_edited_card() {
        let (mut app, ctx, dir, _src, master, ai_px, _scrub) =
            ai_card_fixture("retouch", EditRecipe::default());
        let healed = dir.join("healed.png");
        std::fs::write(&healed, b"png").unwrap();
        let epoch = app.workspace().tools().gen_epoch;
        app.commands().on_retouched(
            &ctx,
            Lang::En,
            epoch,
            Ok((
                image::DynamicImage::ImageRgba8(image::RgbaImage::new(6, 4)),
                RetouchNote::Healed {
                    n: 1,
                    out: healed.clone(),
                    ai_prose: String::new(),
                    notes: Vec::new(),
                    in_face: false,
                },
                healed.clone(),
                RetouchKind::InPlace,
            )),
        );
        assert_eq!(
            strip_kinds(&app),
            vec![VariantKind::Original, VariantKind::Generated, VariantKind::Edited],
            "{}",
            app.workspace().status()
        );
        assert_eq!(app.editor().doc().active, 2);
        assert_eq!(
            app.editor().doc().variants[2].origin.as_deref(),
            Some(healed.as_path()),
            "the retouch baked into the ✎ card"
        );
        assert!(app.editor().doc().variants[2].base.as_ref().is_some_and(|b| !Arc::ptr_eq(b, &ai_px)));
        assert_eq!(
            app.editor().doc().variants[1].origin.as_deref(),
            Some(master.as_path()),
            "the ✨ card keeps its raster"
        );
        assert!(app.editor().doc().variants[1].base.as_ref().is_some_and(|b| Arc::ptr_eq(b, &ai_px)));
    }

    /// Every build through v1.3.2 stored an edit made on the ✨ card AS the
    /// ✨ card: recipe.json holds the edits, the record says the active card
    /// is "generated". The door splits it — once, as unsaved work — and a
    /// strip saved split reopens as it is, silently.
    #[test]
    fn a_strip_saved_with_edits_on_the_ai_card_is_split_at_the_door() {
        let (mut app, ctx, _dir, src, master, ai_px, _scrub) =
            ai_card_fixture("split", EditRecipe { contrast: 7.0, ..Default::default() });
        assert_eq!(
            strip_kinds(&app),
            vec![VariantKind::Original, VariantKind::Generated, VariantKind::Edited],
            "{}",
            app.workspace().status()
        );
        assert_eq!(app.editor().doc().active, 2, "the canvas follows the edits");
        assert_eq!(app.editor().doc().recipe.contrast, 7.0);
        assert!(app.editor().doc().recipe.base_curve.is_empty(), "calibration stripped on AI pixels");
        assert_eq!(app.editor().doc().variants[2].recipe.contrast, 7.0);
        assert!(app.editor().doc().variants[2].base.as_ref().is_some_and(|b| Arc::ptr_eq(b, &ai_px)));
        assert_eq!(app.editor().doc().variants[2].origin.as_deref(), Some(master.as_path()));
        assert!(app.editor().doc().variants[1].recipe.is_noop(), "the ✨ card is pristine");
        assert_eq!(app.editor().doc().variants[1].id, "gen-1", "…and keeps the record's identity");
        assert_eq!(app.editor().doc().variants[1].name.as_deref(), Some("sky"), "…and its name");
        assert!(app.editor().unsaved_marker_dirty(), "the split is unsaved work until Ctrl+S");
        assert_eq!(
            app.workspace().chrome().toasts.iter().filter(|t| t.text.contains("separate cards")).count(),
            1,
            "said once at the door"
        );
        assert_eq!(VariantKind::from_store_str("edited"), Some(VariantKind::Edited));
        assert_eq!(VariantKind::Edited.store_str(), "edited");
        let rec = app.editor().current_strip_record().expect("a three-card strip has a record");
        assert_eq!(rec.active_kind, "edited");

        // Ctrl+S persists the split; the same door then reopens it as it is.
        app.commands().save_xmp();
        assert!(!app.editor().unsaved_marker_dirty(), "{}", app.workspace().status());
        assert_eq!(saved_strip_of(&src).active_kind, "edited");
        let mut again = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {
     src_path: Some(src.clone()),
     ..Default::default()
 }; editor });
        open_onto_ai_card(&mut again, &ctx, &ai_px, &master);
        assert_eq!(
            strip_kinds(&again),
            vec![VariantKind::Original, VariantKind::Generated, VariantKind::Edited],
            "{}",
            again.workspace().status()
        );
        assert_eq!(again.editor().doc().active, 2);
        assert_eq!(again.editor().doc().recipe.contrast, 7.0);
        assert!(again.editor().doc().variants[1].recipe.is_noop());
        assert!(!again.editor().unsaved_marker_dirty(), "a split strip reopens clean: {}", again.workspace().status());
        assert!(
            !again.workspace().chrome().toasts.iter().any(|t| t.text.contains("separate cards")),
            "…and silently"
        );
    }

    /// Ctrl+S on a card over AI pixels saves that card's develop — no
    /// refusal (until 2026-09-13 a generated card was refused outright) —
    /// with the RAW's calibration kept on disk, the canvas stripped, and no
    /// Lightroom projection: the member CLEARS a standing one.
    #[test]
    fn ctrl_s_on_an_ai_pixel_card_saves_its_develop_and_retires_the_projection() {
        let (mut app, _ctx, _dir, src, master, _ai_px, _scrub) =
            ai_card_fixture("save", EditRecipe::default());
        let xp = autoshade::pipeline::xmp_target(&src);
        std::fs::write(
            &xp,
            autoshade::xmp::recipe_to_xmp(&EditRecipe { saturation: 90.0, ..Default::default() }),
        )
        .unwrap();
        // The pristine ✨ card saves as itself.
        app.commands().save_xmp();
        assert!(!app.workspace().status().contains("Reverse-fit"), "no refusal: {}", app.workspace().status());
        assert!(app.workspace().status().contains("AI-generated pixels"), "{}", app.workspace().status());
        assert!(!xp.exists(), "the stale projection is retired by the commit");
        assert!(!app.editor().unsaved_marker_dirty(), "{}", app.workspace().status());
        assert_eq!(saved_strip_of(&src).active_kind, "generated");

        // An edit → ✎; Ctrl+S saves THAT card's develop.
        app.editor_mut().doc_mut().recipe.contrast = 7.0;
        app.commands().save_xmp(); // the boundary fork runs inside
        assert_eq!(
            strip_kinds(&app),
            vec![VariantKind::Original, VariantKind::Generated, VariantKind::Edited],
            "{}",
            app.workspace().status()
        );
        assert!(app.workspace().status().starts_with("recipe saved"), "{}", app.workspace().status());
        let disk: EditRecipe = serde_json::from_str(
            &std::fs::read_to_string(autoshade::store::recipe_target(&src)).unwrap(),
        )
        .unwrap();
        assert_eq!(disk.contrast, 7.0);
        assert_eq!(
            disk.base_curve,
            vec![[0.0, 0.0], [0.5, 0.62], [1.0, 1.0]],
            "the disk form keeps the RAW's saved calibration"
        );
        assert_eq!(disk.as_shot_k, Some(5653.0));
        assert!(app.editor().doc().recipe.base_curve.is_empty(), "the canvas stays stripped");
        let (origin, generated) = autoshade::store::read_pixel_source(&src).unwrap();
        assert!(generated);
        assert_eq!(origin, master);
        let rec = saved_strip_of(&src);
        assert_eq!(rec.active_kind, "edited");
        assert_eq!(
            rec.others.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(),
            vec!["original", "generated"]
        );
        assert!(!xp.exists(), "still no projection over AI pixels");
        assert!(!app.editor().unsaved_marker_dirty(), "{}", app.workspace().status());
    }

    /// The reverse-fit reads the ✨ card's raster, never an ✎ card: the fit
    /// solves pixels, and the edits are sliders the user already has.
    #[test]
    fn fit_target_is_the_pristine_ai_cards_raster_never_the_edited_cards() {
        let (mut app, ctx, _dir, _src, master, _ai_px, _scrub) =
            ai_card_fixture("fit", EditRecipe::default());
        assert_eq!(
            app.editor().fit_target(&app.workspace()).as_deref(),
            Some(master.as_path()),
            "premise: the ✨ card's raster is the default target"
        );
        app.editor_mut().doc_mut().recipe.contrast = 7.0;
        assert!(app.commands().fork_edited_card());
        assert_eq!(app.editor().fit_target(&app.workspace()), None, "an ✎ card is not a fit target");
        app.commands().switch_variant(1, &ctx);
        assert_eq!(app.editor().fit_target(&app.workspace()).as_deref(), Some(master.as_path()));
    }

    /// 2026-09-13 (the user's own store: ▣ `origin = …denoise.png`, the ◭
    /// card active with `origin = None`, no `pixels.json`): the ◭ card a fit
    /// lands as hangs off the NEGATIVE's in-place master — the pixels the ▣
    /// card develops and the fit was solved on — sharing the ▣ card's decoded
    /// base, and the ● pixel mirror follows the link the worker persisted.
    /// Without a master the fit keeps landing on the loaded photo, origin-free.
    #[test]
    fn a_reverse_fit_lands_on_the_negatives_master() {
        let ctx = egui::Context::default();
        let mut app = AutoShadeApp::default();
        let master = std::path::PathBuf::from("out/_negative_master_test.denoise.png");
        let clean = std::sync::Arc::new(image::DynamicImage::new_rgba8(4, 4));
        let ai = std::sync::Arc::new(image::DynamicImage::new_rgba8(4, 4));
        app.editor_mut().doc_mut().variants = vec![
            Variant {
                id: ORIGINAL_VARIANT_ID.into(),
                name: None,
                kind: VariantKind::Original,
                recipe: EditRecipe::default(),
                base: Some(clean.clone()),
                origin: Some(master.clone()),
                thumb: None,
            },
            Variant {
                id: "gen".into(),
                name: None,
                kind: VariantKind::Generated,
                recipe: EditRecipe::default(),
                base: Some(ai.clone()),
                origin: Some(std::path::PathBuf::from("out/_negative_master_test.reimagine.png")),
                thumb: None,
            },
        ];
        app.editor_mut().doc_mut().active = 1;
        app.editor_mut().photo_mut().base_preview = Some(ai.clone());
        assert_eq!(
            app.commands().negative_origin().as_deref(),
            Some(master.as_path()),
            "the ▣ card's master is the negative even while the ✨ card is active"
        );
        let outcome = |negative: Option<std::path::PathBuf>| FitOutcome {
            recipe: EditRecipe { contrast: 9.0, ..Default::default() },
            err_before: 0.3,
            err_after: 0.1,
            rationale_notes: Vec::new(),
            status: Vec::new(),
            persisted: true,
            negative,
        };
        app.commands().on_fitted(&ctx, Lang::En, Box::new(Ok(outcome(Some(master.clone())))));
        let fitted = &app.editor().doc().variants[app.editor().doc().active];
        assert_eq!(fitted.kind, VariantKind::Fitted);
        assert_eq!(
            fitted.origin.as_deref(),
            Some(master.as_path()),
            "the ◭ card hangs off the negative's master"
        );
        assert!(
            fitted.base.as_ref().is_some_and(|b| std::sync::Arc::ptr_eq(b, &clean)),
            "…and shares the ▣ card's decoded pixels"
        );
        assert_eq!(
            app.editor().session().hist.pixels_on_disk.as_deref(),
            Some(master.as_path()),
            "the ● pixel mirror follows the persisted link"
        );
        assert_eq!(app.editor().doc().recipe.contrast, 9.0, "the fit's develop is live on the new card");

        // No master on the negative: the fit lands on the loaded photo, as
        // it always did.
        app.editor_mut().doc_mut().variants[0].origin = None;
        app.editor_mut().doc_mut().variants[0].base = None;
        app.commands().switch_variant(1, &ctx);
        assert_eq!(app.commands().negative_origin(), None);
        app.commands().on_fitted(&ctx, Lang::En, Box::new(Ok(outcome(None))));
        let fitted = &app.editor().doc().variants[app.editor().doc().active];
        assert_eq!(fitted.kind, VariantKind::Fitted);
        assert_eq!(fitted.origin, None);
        assert_eq!(app.editor().session().hist.pixels_on_disk, None);
    }

    /// 2026-09-15 (user decision): an AI denoise is NOT an in-place retouch
    /// any more — it lands as a NEW ◈ Denoised negative card whose origin is
    /// the denoised master and whose recipe is a copy of the card it was run
    /// from, auto-switched to. The ▣ card keeps its pixels, its base and its
    /// `origin = None` (the old in-place landing redefined the ▣ card and
    /// left the untouched negative unreachable). The negative then IS the ◈
    /// card's master (`negative_origin`), and `negative_path` — what the
    /// reimagine develops from — follows it.
    #[test]
    fn an_ai_denoise_lands_as_a_new_denoised_card_and_leaves_the_source_and_negative_alone() {
        let ctx = egui::Context::default();
        let src = std::path::PathBuf::from("_negative_follow_test.ARW");
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {
     src_path: Some(src.clone()),
     ..Default::default()
 }; editor });
        let b0 = std::sync::Arc::new(image::DynamicImage::new_rgba8(4, 4));
        let developed = EditRecipe { contrast: 7.0, ..Default::default() };
        app.editor_mut().doc_mut().variants = vec![Variant {
            id: ORIGINAL_VARIANT_ID.into(),
            name: None,
            kind: VariantKind::Original,
            recipe: developed.clone(),
            base: Some(b0.clone()),
            origin: None,
            thumb: None,
        }];
        app.editor_mut().doc_mut().active = 0;
        app.editor_mut().doc_mut().recipe = developed.clone();
        app.editor_mut().photo_mut().base_preview = Some(b0.clone());
        { let (e, w, _l) = app.editor_workspace_mut(); e.reset_history(&*w) };
        assert_eq!(app.commands().negative_origin(), None);
        assert_eq!(app.commands().negative_path().as_deref(), Some(src.as_path()), "no master: the loaded file");
        let out = std::path::PathBuf::from("out/_negative_follow_test.denoise.png");
        let epoch = app.workspace().tools().gen_epoch;
        app.commands().on_retouched(
            &ctx,
            Lang::En,
            epoch,
            Ok((
                image::DynamicImage::new_rgba8(4, 4),
                RetouchNote::Denoised { out: out.clone(), on_mosaic: true },
                out.clone(),
                RetouchKind::NewDenoised,
            )),
        );
        assert_eq!(
            strip_kinds(&app),
            vec![VariantKind::Original, VariantKind::Denoised],
            "{}",
            app.workspace().status()
        );
        assert_eq!(app.editor().doc().active, 1, "the new ◈ card is under the canvas");
        assert_eq!(app.editor().doc().variants[1].origin.as_deref(), Some(out.as_path()), "…hanging off the denoised master");
        assert_eq!(app.editor().doc().variants[1].recipe, developed, "…carrying the source card's develop");
        assert!(app.editor().doc().variants[1].kind.is_source_based(), "a ◈ card is parametric over its master");
        assert!(!app.editor().doc().variants[1].kind.on_ai_pixels(), "…and carries no AI-pixel caveats");
        assert_eq!(app.editor().doc().variants[0].recipe, developed, "the ▣ card keeps its develop");
        assert!(
            app.editor().doc().variants[0].base.as_ref().is_some_and(|b| std::sync::Arc::ptr_eq(b, &b0)),
            "…and its base"
        );
        assert_eq!(app.editor().doc().variants[0].origin, None, "…and its untouched pixel source");
        assert_eq!(app.commands().negative_origin().as_deref(), Some(out.as_path()), "the negative is now the ◈ master");
        assert_eq!(app.commands().negative_path().as_deref(), Some(out.as_path()), "the reimagine input follows");
        assert_eq!(
            app.editor().active_source_path().as_deref(),
            Some(out.as_path()),
            "and the ◈ card's own source is the same file — one negative"
        );
        assert!(app.workspace().status().contains("new ◈ card"), "{}", app.workspace().status());
        assert!(app.workspace().status().contains("sensor mosaic"), "{}", app.workspace().status());
    }

    /// A ◈ card round-trips through the strip record: the store admits the
    /// "denoised" spelling (`known_variant_kind`), the record carries its
    /// master as `origin`, and the spelling maps back to the kind.
    #[test]
    fn a_denoised_card_round_trips_through_the_strip_record() {
        let dir = std::env::temp_dir()
            .join(format!("autoshade-gui-denoised-card-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("_gui_denoised_card.ARW");
        let dev = autoshade::store::develop_dir(&src);
        let _ = std::fs::remove_dir_all(&dev);
        std::fs::create_dir_all(&dev).unwrap();
        let _scrub = Scrub(vec![dir.clone(), dev.clone()]);
        let master = dir.join("denoise.png");
        let developed = EditRecipe { contrast: 4.0, ..Default::default() };
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {
     src_path: Some(src.clone()),
     ..Default::default()
 }; editor });
        app.editor_mut().doc_mut().variants = vec![
            Variant {
                id: ORIGINAL_VARIANT_ID.into(),
                name: None,
                kind: VariantKind::Original,
                recipe: developed.clone(),
                base: None,
                origin: None,
                thumb: None,
            },
            Variant {
                id: "dn-1".into(),
                name: Some("clean".into()),
                kind: VariantKind::Denoised,
                recipe: developed.clone(),
                base: None,
                origin: Some(master.clone()),
                thumb: None,
            },
        ];
        app.editor_mut().doc_mut().active = 1;
        app.editor_mut().doc_mut().recipe = developed.clone();
        assert_eq!(VariantKind::Denoised.store_str(), "denoised");
        assert_eq!(VariantKind::from_store_str("denoised"), Some(VariantKind::Denoised));
        let rec = app.editor().current_strip_record().expect("a two-card strip has a record");
        assert_eq!(rec.active_kind, "denoised");
        assert_eq!(rec.active_id.as_deref(), Some("dn-1"));
        assert_eq!(rec.active_name.as_deref(), Some("clean"));
        app.editor_mut().persist_strip(&src).expect("strip persists");
        let saved = saved_strip_of(&src);
        assert_eq!(saved.active_kind, "denoised");
        assert_eq!(saved.active_pos, 1);
        assert_eq!(saved.others.len(), 1);
        assert_eq!(saved.others[0].kind, "original");
        assert_eq!(saved.others[0].origin, None, "the ▣ card carries no master of its own");
        // The active card's master travels in pixels.json, the way every
        // origin-carrying active card's does, flagged by `on_ai_pixels` — and
        // a ◈ master is a camera frame, not AI pixels: it keeps calibration,
        // projects to a Lightroom XMP and never earns the AI caveats.
        assert!(!VariantKind::Denoised.on_ai_pixels());
    }

    /// v1.5.0 Track S: a stack lands as a NEW ▦ card on the ◈ card's terms —
    /// the merged master is its `origin`, the develop it was run from is its
    /// recipe, the frame it was made from keeps its own pixels — and it
    /// becomes the negative. An HDR merge hands over the stops it recovered on
    /// top of that: they arrive as the SDR rendition stage turned ON with that
    /// much room, because without them the section would offer a range the
    /// frame does not have, and the recovered highlights would be unreachable.
    ///
    /// MUTATION: `NewStacked` → `NewGenerated` in start_stack; drop the
    /// headroom transfer in the landing; `is_remade_negative` back to ◈ alone.
    #[test]
    fn a_stack_lands_as_a_new_stacked_card_that_becomes_the_negative() {
        let ctx = egui::Context::default();
        let src = std::path::PathBuf::from("_stack_landing_test.ARW");
        let developed = EditRecipe { contrast: 11.0, ..Default::default() };
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {

                src_path: Some(src.clone()),

                ..Default::default()

            }; *editor.doc_mut() = Document {


                variants: vec![Variant {
                id: ORIGINAL_VARIANT_ID.into(),
                name: None,
                kind: VariantKind::Original,
                recipe: developed.clone(),
                base: None,
                origin: None,
                thumb: None,
            }],


                recipe: developed.clone(),


                ..Default::default()


            }; editor });
        { let (e, w, _l) = app.editor_workspace_mut(); e.reset_history(&*w) };
        let out = std::path::PathBuf::from("out/_stack_landing_test.stack.png");
        let epoch = app.workspace().tools().gen_epoch;
        app.commands().on_retouched(
            &ctx,
            Lang::En,
            epoch,
            Ok((
                image::DynamicImage::new_rgba8(4, 4),
                RetouchNote::Stacked {
                    out: out.clone(),
                    kind: autoshade::stack::merge::StackKind::Hdr,
                    frames: 3,
                    travel: 4.2,
                    uncovered: 0.0125,
                    headroom_ev: 3.75,
                },
                out.clone(),
                RetouchKind::NewStacked,
            )),
        );
        assert_eq!(
            strip_kinds(&app),
            vec![VariantKind::Original, VariantKind::Stacked],
            "{}",
            app.workspace().status()
        );
        assert_eq!(app.editor().doc().active, 1, "the new ▦ card is under the canvas");
        let card = &app.editor().doc().variants[1];
        assert_eq!(card.origin.as_deref(), Some(out.as_path()), "…hanging off the merged master");
        assert_eq!(card.recipe.contrast, developed.contrast, "…carrying the develop it was run from");
        assert!(card.kind.is_source_based(), "a ▦ master is a camera frame, not AI pixels");
        assert!(!card.kind.on_ai_pixels(), "…so it earns none of the AI caveats");
        assert!(card.kind.is_remade_negative(), "…and it is the negative, remade");
        assert!(card.recipe.hdr_edit, "the recovered stops must reach the rendition stage");
        assert!((card.recipe.hdr_max_ev - 3.75).abs() < 1e-6, "…with the room the merge measured");
        assert_eq!(app.editor().doc().variants[0].recipe, developed, "the frame it was made from keeps its develop");
        assert_eq!(app.editor().doc().variants[0].origin, None, "…and its untouched pixel source");
        assert_eq!(
            app.commands().negative_origin().as_deref(),
            Some(out.as_path()),
            "the negative is now the ▦ master"
        );
        assert_eq!(VariantKind::Stacked.store_str(), "stacked");
        assert_eq!(VariantKind::from_store_str("stacked"), Some(VariantKind::Stacked));
        // The landing line names the merge that ran and carries the three
        // numbers worth reading before trusting a stack.
        for want in ["3 frames", "HDR merge", "new ▦ card", "4.2 px", "1.25%", "3.75 EV"] {
            assert!(app.workspace().status().contains(want), "{want} missing from: {}", app.workspace().status());
        }
    }

    /// 「🤖 AI Denoise on export」 sits out on a ◈ card — its master is already
    /// the denoised negative; a second pass would only smooth it further —
    /// and the export echo says so by carrying no amount.
    #[test]
    fn the_export_denoise_is_skipped_on_a_denoised_card() {
        let card = |kind: VariantKind| Variant {
            id: "c".into(),
            name: None,
            kind,
            recipe: EditRecipe::default(),
            base: None,
            origin: None,
            thumb: None,
        };
        let mut app = AutoShadeApp::default().but_workspace({ let mut view = WorkspaceView::default(); *view.exp_mut() = ExportState {
                save_denoise: true,
                save_denoise_strength: 0.35,
                ..Default::default()
            }; view });
        for kind in [VariantKind::Original, VariantKind::Generated, VariantKind::Fitted, VariantKind::Edited] {
            app.editor_mut().doc_mut().variants = vec![card(kind)];
            app.editor_mut().doc_mut().active = 0;
            assert!(app.editor().export_denoise_applies(&app.workspace()), "{kind:?}");
            assert!(app.editor().export_summary(&app.workspace(), Lang::En).contains("AI Denoise 35%"), "{kind:?}");
        }
        app.editor_mut().doc_mut().variants = vec![card(VariantKind::Denoised)];
        assert!(!app.editor().export_denoise_applies(&app.workspace()));
        assert!(!app.editor().export_summary(&app.workspace(), Lang::En).contains("AI Denoise"), "{}", app.editor().export_summary(&app.workspace(), Lang::En));
        app.workspace_mut().exp_mut().save_denoise = false;
        app.editor_mut().doc_mut().variants = vec![card(VariantKind::Original)];
        assert!(!app.editor().export_denoise_applies(&app.workspace()), "unticked is unticked on any card");
        // The render reads the gate, not the raw checkbox: `export_summary`'s
        // line above and `export_denoise_applies` are the same two answers the
        // render asks for, so the behaviour is already on the table.
    }

    /// 「＋」 has nothing to snapshot on the pristine ✨ card and everything
    /// on the ✎ card — attributed to it.
    #[test]
    fn a_version_snapshot_is_refused_on_the_pristine_ai_card_and_taken_on_the_edited_one() {
        let (mut app, _ctx, _dir, src, _master, _ai_px, _scrub) =
            ai_card_fixture("snapshot", EditRecipe::default());
        app.commands().save_version();
        assert!(app.workspace().status().contains("pristine"), "{}", app.workspace().status());
        assert!(!autoshade::store::version_target(&src, 1).exists());
        app.editor_mut().doc_mut().recipe.contrast = 7.0;
        app.commands().save_version(); // forks at the boundary, then snapshots the ✎ card
        assert_eq!(app.editor().doc().variants[app.editor().doc().active].kind, VariantKind::Edited);
        let v1 = autoshade::store::version_target(&src, 1);
        assert!(v1.exists(), "{}", app.workspace().status());
        let snap: EditRecipe = serde_json::from_str(&std::fs::read_to_string(&v1).unwrap()).unwrap();
        assert_eq!(snap.contrast, 7.0);
        let meta = autoshade::store::read_version_meta(&src);
        assert_eq!(
            meta.iter().find(|m| m.n == 1).and_then(|m| m.from_kind.as_deref()),
            Some("edited")
        );
    }

    /// 「▣ apply to Original」 refuses both AI-pixel kinds, each for its own
    /// reason.
    #[test]
    fn apply_to_original_refuses_an_edited_card_with_its_own_reason() {
        let (mut app, ctx, _dir, _src, _master, _ai_px, _scrub) =
            ai_card_fixture("apply", EditRecipe::default());
        app.commands().apply_to_original(1, &ctx);
        assert!(app.workspace().status().contains("lives in its pixels"), "{}", app.workspace().status());
        app.editor_mut().doc_mut().recipe.contrast = 7.0;
        assert!(app.commands().fork_edited_card());
        app.commands().apply_to_original(2, &ctx);
        assert!(app.workspace().status().contains("AI-generated pixels"), "{}", app.workspace().status());
        assert_eq!(app.editor().doc().variants[0].recipe.contrast, 4.0, "the ▣ card is untouched");
    }

    /// The choices that live inside a worker or inside eframe's `update()`
    /// cannot be driven headlessly, so they are read off the source — ONE
    /// census for all of them, so a worker fact failing does not turn a
    /// behaviour test red.
    ///
    /// MUTATION coverage: fill `layer: true` / `RetouchKind::Layer`; adjust
    /// `RetouchKind::NewGenerated` and its two arms; the denoise's
    /// `NewDenoised` and full frame; stack `NewStacked` and its inputs; the
    /// fit's negative/pixel capture and the reimagine argument; the preview's
    /// engine tail; the film edge on six develops; the frame hook before the
    /// develop dispatch.
    #[test]
    fn the_worker_and_frame_choices_are_pinned_in_one_census() {
        fn develops_at_film(src: &str, head: &str, edge: &str, develop: &str) {
            let body = &src[src.find(head).unwrap_or_else(|| panic!("{head} moved"))..];
            let rest = &body[head.len()..];
            let next = ["\n    pub(crate) fn ", "\npub(crate) fn ", "\npub fn ", "\nfn "];
            let end = next.iter().filter_map(|m| rest.find(m)).min().map_or(body.len(), |i| i + head.len());
            let body = &body[..end];
            let call = &body[body.find(develop).unwrap_or_else(|| panic!("{head}: no {develop}"))..];
            let args = &call[develop.len()..call.find(';').unwrap_or(call.len())];
            assert!(body.contains(edge), "{head} no longer reads the source's film edge");
            assert!(args.contains("film"), "{head} develops at raster scale: {args}");
        }
        let src = gui_source();

        // The frame hook sits between the panels (where the sliders write the
        // recipe) and the develop dispatch.
        let panels = src.find("self.upd_strips_and_side_panels(ctx);").expect("the panels call");
        let hook = src[panels..]
            .find("self.commands().fork_edited_card();")
            .map(|i| i + panels)
            .expect("the frame hook after the panels");
        let dispatch = src.find("if self.editor.session().dirty && !self.editor.scheduler().inflight {").expect("the dispatch");
        assert!(hook < dispatch, "the hook runs before the develop is dispatched");

        // The fill develops the card's picture for the model, asks the
        // library for a LAYER and lands as one.
        let fill = &src[src.find("pub(crate) fn start_fill(").expect("start_fill moved")..];
        let fill = &fill[..fill.find("pub(crate) fn start_heal(").expect("start_heal moved")];
        assert!(fill.contains("autoshade::generative::retouch_onto("), "the fill no longer hands the library its own base");
        assert!(
            fill.contains("developed_card_pixels(&path, &recipe, full_res)"),
            "the fill no longer develops the card's picture for the model"
        );
        assert!(fill.contains("layer: true,"), "the fill no longer asks for a layer");
        assert!(fill.contains("RetouchKind::Layer))"), "the fill no longer lands as a layer");
        assert!(!fill.contains("RetouchKind::InPlace"), "the fill went back to an in-place landing");

        // The adjust captures the card's raster and recipe at the click, runs
        // the fill path for strokes and the whole-image path otherwise.
        let start = src.find("pub(crate) fn start_adjust(").expect("start_adjust moved");
        let adjust = &src[start..src[start..].find("pub(crate) fn start_heal(").unwrap() + start];
        assert!(adjust.contains("self.editor.active_source_path()"), "the card supplies its own raster");
        assert!(adjust.contains("self.editor.doc().recipe.clone()"), "the live recipe is captured at the click");
        assert!(adjust.contains("developed_card_pixels(&path, &recipe, false)"));
        let split = adjust.find("if let Some(mask_png) = mask_png").unwrap();
        let arms = &adjust[split..];
        let whole = arms.find("} else {").unwrap();
        assert!(arms[..whole].contains("autoshade::generative::retouch_onto("), "strokes take the fill path");
        assert!(arms[whole..].contains("autoshade::generative::adjust_onto("), "no strokes take the whole-image path");
        assert!(adjust.contains("RetouchKind::NewGenerated))"));
        assert!(!adjust.contains("RetouchKind::InPlace"));

        // The denoise lands as a NEW card on the full frame; the ≤2048 px
        // working-copy tier and the Full-res checkbox are gone.
        let denoise = &src[src.find("pub(crate) fn start_ai_denoise(").expect("start_ai_denoise moved")..];
        let denoise = &denoise[..denoise.find("pub(crate) fn start_clone(").expect("start_clone moved")];
        assert!(denoise.contains("RetouchKind::NewDenoised))"), "the denoise no longer lands as a new card");
        assert!(!denoise.contains("RetouchKind::InPlace"), "the denoise went back to an in-place landing");
        assert!(denoise.contains("denoise_active(&opts, &path, &part, &out)"), "the denoise no longer runs the full frame");
        assert!(!denoise.contains("2048"), "the ≤2048 px working-copy tier came back");
        assert!(!denoise.contains("denoise_fullres"), "the retired Full-res checkbox came back");

        // The fit reads the negative once at the click and the persist links
        // it; the reimagine develops the negative, master included.
        assert!(src.contains("let negative = self.fit_negative();"));
        assert!(
            src.contains("(Some(p), Some(master)) => {") && src.contains("fit_source_baked(master, pixels)"),
            "the solve's source frame is the master when the negative carries one"
        );
        assert!(
            src.contains("pixels: match negative.as_deref() {"),
            "the persisted pixel link follows the same capture"
        );
        assert!(src.contains("let Some(negative) = self.negative_path() else { return };"));
        assert!(src.contains("&cfg, &negative, &prompt, \"high\""), "the reimagine develops the negative, master included");

        // The stack lands as a NEW card, the card's frame is the reference
        // and the stack runs on the full frame.
        let stack = &src[src.find("pub(crate) fn start_stack(").expect("start_stack moved")..];
        let stack = &stack[..stack.find("pub(crate) fn start_clone(").expect("start_clone moved")];
        assert!(stack.contains("RetouchKind::NewStacked,"), "the stack no longer lands as a new card");
        assert!(stack.contains("let mut inputs = vec![path.clone()];"), "this card's frame is no longer the stack's reference");
        assert!(stack.contains("stack_files(&inputs, &part, &opts, None, &out)"), "the stack no longer runs on the full frame");

        // The canvas preview runs the engine's tail and keeps the whole frame.
        let preview = &src[src.find("pub(crate) fn build_preview(").expect("build_preview moved")..];
        let preview = &preview[..preview.len().min(2400)];
        assert!(preview.contains("autoshade::render::frame_and_finish("), "the canvas preview no longer runs the engine's tail");
        assert!(preview.contains("autoshade::render::CropPolicy::Keep"), "the canvas must keep the whole frame and only POSITION the finish");
        assert!(!preview.contains("autoshade::render::apply_lens_geometry("), "a second copy of the geometry chain is exactly what the tail replaced");

        // Every surface that shows a develop reads the source's film edge.
        let framed = "develop_preview_framed(";
        develops_at_film(src, "pub(crate) fn refresh_mask_overlay(", "self.editor.film_short_edge()", framed);
        develops_at_film(src, "pub(crate) fn handle_range_pick(", "self.editor.film_short_edge()", framed);
        develops_at_film(src, "pub(crate) fn handle_point_color_pick(", "self.editor.film_short_edge()", framed);
        develops_at_film(src, "fn developed_card_pixels(", "autoshade::decode::film_short_edge(path)", framed);
        develops_at_film(src, "pub(crate) fn start_redevelop(", "self.editor.film_short_edge()", "build_preview(");
        develops_at_film(src, "pub(crate) fn build_preview(", "film_short_edge", "develop_preview_film(");
        let serve = include_str!("../../../serve.rs");
        develops_at_film(serve, "fn api_develop(", "decode::film_short_edge(&src)", "develop_preview_film(");

        // The toolbar and the window's own decorations agree on the custom
        // chrome; a frame that decorates itself while buttons are drawn over
        // it would leave both sets on screen.
        assert!(
            src.contains(".with_decorations(!crate::window_chrome::CUSTOM_CHROME)"),
            "the frame and the drawn buttons must agree"
        );

        // The reverse-fit prompt and its panel share ONE source menu builder.
        let body = |head: &str| -> &str {
            let from = src.find(head).unwrap_or_else(|| panic!("{head} moved"));
            let rest = &src[from..];
            &rest[..rest.find("\n    }").unwrap()]
        };
        assert!(body("fn ai_reverse_fit(").contains("self.fit_source_menu()"), "the panel builds its own source list again");
        let layer = body("pub(crate) fn fit_prompt_layer(");
        assert!(layer.contains("self.fit_source_menu()"), "the prompt builds its own source list again");
        assert!(layer.contains("self.start_fit()"), "the prompt runs a fit of its own");

        // Each denoise verb reads ITS fold's dial and no literal strength.
        let now = &src[src.find("pub(crate) fn start_ai_denoise(").expect("start_ai_denoise moved")..];
        let now = &now[..now.find("pub(crate) fn start_clone(").expect("start_clone moved")];
        assert!(now.contains("let strength = self.editor.selected_denoise_strength(&self.workspace, false);"));
        assert!(now.contains("DenoiseOpts::from_config(&cfg, None, strength)"));
        assert!(!now.contains("save_denoise_strength"), "「AI Denoise now」 must not read the Export dial");
        let export = &src[src.find("pub(crate) fn start_render_to(").expect("start_render_to moved")..];
        let export = &export[..export.find("\n    pub(crate) fn ").unwrap()];
        assert!(export.contains("let denoise_strength = self.editor.selected_denoise_strength(&self.workspace, true);"));
        assert!(!export.contains("self.workspace.exp().denoise_strength"), "the export must not read the Detail dial");
        for (name, text) in [("start_ai_denoise", now), ("start_render_to", export)] {
            assert!(
                !text.contains("None, 1.0)") && !text.contains("None, 0.5)"),
                "{name}: a literal strength bypasses the dial"
            );
        }

        // The reverse-fit block reads its own dial at both entry points; the
        // Analysis dial is read once, in `analysis_strength`.
        let worker = &src[src.find("pub(crate) fn start_fit(").expect("start_fit moved")..];
        let worker = &worker[..worker.find("\n    pub(crate) fn ").unwrap()];
        assert!(worker.contains("let fit_strength = self.aip.fit_strength();"), "the reverse-fit block must read its own dial through fit_strength()");
        assert_eq!(worker.matches("strength: fit_strength,").count(), 2, "both fit entry points (zoned and global) must receive the fit's dial");
        let fit_strength = &src[src.find("pub(crate) fn fit_strength(").expect("fit_strength moved")..];
        assert_eq!(fit_strength.matches("GradeStrength::new(self.fit_strength)").count(), 1, "exactly one reading of the fit's dial: fit_strength() itself");
        let analysis_strength = &src[src.find("pub(crate) fn analysis_strength(").expect("analysis_strength moved")..];
        assert_eq!(analysis_strength.matches("GradeStrength::new(self.grade_strength)").count(), 1, "exactly one reading of the Analysis dial: analysis_strength() itself");
        assert!(!src.contains("fn panel_strength") && !src.contains("self.panel_strength()"), "no shared reading is left (the name may survive in prose, never as code)");

        // Both range-reference callers go through the one builder.
        for head in ["pub(crate) fn refresh_mask_overlay(", "pub(crate) fn handle_range_pick("] {
            let body = &src[src.find(head).unwrap_or_else(|| panic!("{head} moved"))..];
            let end = body[head.len()..].find("\n    pub(crate) fn ").expect("the next method") + head.len();
            assert!(body[..end].contains("self.editor.range_reference_recipe("), "{head} builds its own reference copy again");
        }

        // A retouch landing clears through `consumed_area`, never by hand.
        assert!(src.contains("if let Some(owner) = note.consumed_area()"), "the landing clears through consumed_area");

        // Every window in the frame carries a fixed egui id: the localised
        // title would reset its state on a language switch.
        let mut seen = 0;
        for (start, _) in src.match_indices("Window::new(") {
            let end = src[start..].find(".show(ctx").expect("the window's show call") + start;
            let builder = &src[start..end];
            assert!(
                builder.contains(".id(Id::new(") || builder.contains(".id(egui::Id::new("),
                "a window whose id is its localised title, so its state resets with the language"
            );
            seen += 1;
        }
        assert!(seen >= 2, "premise: the Settings and Shortcuts windows are both built here ({seen} found)");

        // The pipeline READS the request's embedding switch instead of
        // resolving its own; a behavioural assertion is out of reach (paid
        // network chain), so the old door is gone and the new one is the only read.
        let pipeline = include_str!("../../../pipeline.rs");
        assert!(!pipeline.contains("embedding_effective"), "the pipeline must not resolve the switch itself");
        assert_eq!(pipeline.matches("req.embed.on()").count(), 1, "exactly one read of the request's switch");

        // Normal's "first download" size is the sum the denoise script really pulls.
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
