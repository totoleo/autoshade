// One part of the GUI's tests (src/bin/gui/tests.rs includes it): turned photos, the two subject backends, preference round trips, the storage key, button rows, window ids, the paint canvas and the side panel cells.

    /// R27 A6 — the toolbar rotate is a RECIPE edit, and the base plates
    /// follow it through `sync_base_turns` rather than through a rotation
    /// stage in the canvas. Everything downstream (`view_norm_to_orig`, the
    /// crop handles, the coverage overlay, the paint canvas) is defined
    /// against the plate, so turning the pixels turns all of them at once.
    ///
    /// Drives the pure pieces with a headless egui context — never
    /// `run_native`.
    ///
    /// MUTATION THIS CATCHES: make `sync_base_turns` turn by
    /// `recipe.quarter_turns` instead of by the delta against `base_turns` and
    /// the SECOND turn below over-rotates (the plate ends 6x8, not 8x6) —
    /// the double-application hazard, on the GUI side of the fence.
    #[test]
    fn the_toolbar_turn_moves_the_recipe_and_the_plate_together() {
        let (mut app, _scrub) = app_with_masked_photo("rotate-plate");
        let ctx = egui::Context::default();
        // A deliberately non-square plate, so a turn is visible in the dims.
        let plate = std::sync::Arc::new(image::DynamicImage::new_rgb8(8, 6));
        app.editor_mut().photo_mut().base_preview = Some(plate.clone());
        app.editor_mut().photo_mut().source_preview = Some(plate);
        app.editor_mut().photo_mut().base_turns = 0;
        let src = std::env::temp_dir().join(format!("autoshade-rotate-plate-{}.arw", std::process::id()));
        std::fs::write(&src, b"raw").unwrap();
        app.editor_mut().photo_mut().src_path = Some(src);

        assert!(app.editor().can_rotate(&app.workspace()), "a parametric photo with a plate is rotatable");
        { let (e, w, l) = app.editor_workspace_mut(); e.rotate_photo(w, l, 1, &ctx) };
        assert_eq!(app.editor().doc().recipe.quarter_turns, 1);
        assert_eq!(app.editor().photo().base_turns, 1);
        assert_eq!(
            app.editor().photo().base_preview.as_ref().unwrap().dimensions(),
            (6, 8),
            "one clockwise quarter turn transposes the plate"
        );
        assert_eq!(app.editor().photo().source_preview.as_ref().unwrap().dimensions(), (6, 8));

        // A second turn moves by the DELTA, not by the running total.
        { let (e, w, l) = app.editor_workspace_mut(); e.rotate_photo(w, l, 1, &ctx) };
        assert_eq!(app.editor().doc().recipe.quarter_turns, 2);
        assert_eq!(app.editor().photo().base_preview.as_ref().unwrap().dimensions(), (8, 6));

        // An UNDO changes only the recipe; the per-frame reconciler is what
        // brings the plate back — one mover for both directions.
        { let (e, w, l) = app.editor_workspace_mut(); e.undo(w, l, &ctx) };
        assert_eq!(app.editor().doc().recipe.quarter_turns, 1, "one Ctrl+Z per turn (7.7)");
        { let (e, w, l) = app.editor_workspace_mut(); e.sync_base_turns(w, l, &ctx) };
        assert_eq!(app.editor().photo().base_preview.as_ref().unwrap().dimensions(), (6, 8));
        assert_eq!(app.editor().photo().base_turns, 1);

        // Idempotent: the common per-frame call is a u8 compare that moves
        // nothing (a reconciler that turned on every frame would spin the
        // canvas).
        { let (e, w, l) = app.editor_workspace_mut(); e.sync_base_turns(w, l, &ctx) };
        assert_eq!(app.editor().photo().base_preview.as_ref().unwrap().dimensions(), (6, 8));

        // BAKED PIXELS close the door, whole strip (a limitation by design):
        // a master raster is a file in the frame it was baked in, and this
        // build cannot record that it predates a turn.
        app.editor_mut().doc_mut().variants[0].origin = Some("master.png".into());
        assert!(!app.editor().can_rotate(&app.workspace()), "a baked master must block the turn, not be turned under");
        let before = app.editor().doc().recipe.quarter_turns;
        { let (e, w, l) = app.editor_workspace_mut(); e.rotate_photo(w, l, 1, &ctx) };
        assert_eq!(app.editor().doc().recipe.quarter_turns, before, "a refused turn changes nothing");
    }

    /// R29 C3/C4 item 1 — the GUI tells the two subject backends apart, and it
    /// does so in TWO SENTENCES, not one.
    ///
    /// R29 B4 pinned BiRefNet and kept U²-Net as a named fallback tier, then
    /// registered exactly this hole: the sidecar announced the degradation on
    /// stderr, which a `windows_subsystem = "windows"` build has no console to
    /// receive, so both tiers produced the same "AI mask added" line. A
    /// photographer whose machine had quietly dropped to the fallback — softer
    /// edges, no strand structure on hair, a subject invented on a landscape
    /// that has none — could not tell from anything on screen.
    ///
    /// MUTATION-LINED: dropping the backend from `Msg::Segmented`, dropping the
    /// status suffix, or raising the same toast (or none) for both tiers fails
    /// one of the four assertions below. The pair is deliberate — a test that
    /// only checked the fallback would pass on a build that warned about
    /// everything.
    #[test]
    fn the_two_subject_backends_land_as_two_different_sentences() {
        use std::path::PathBuf;
        let ctx = egui::Context::default();

        // THE PINNED MODEL: named in the status, and no alarm raised.
        let mut ok = AutoShadeApp::default().but_workspace({ let mut view = WorkspaceView::default(); *view.busy_mut() = true; view });
        ok.services().tx
            .send(Msg::Segmented(Ok((
                "Subject".into(),
                PathBuf::from("mask-subject.png"),
                "BiRefNet e2bf8e4460fc".into(),
            ))))
            .unwrap();
        ok.poll_workers(&ctx);
        assert!(
            ok.workspace().status().contains("BiRefNet e2bf8e4460fc"),
            "the run must name the model that drew the mask: {}",
            ok.workspace().status()
        );
        assert!(
            !ok.workspace().chrome().toasts.iter().any(|t| matches!(t.kind, ToastKind::Error)),
            "the model the user ruled for is not a warning"
        );
        assert_eq!(ok.editor().doc().recipe.masks.len(), 1, "the mask still lands");

        // THE FALLBACK TIER: named in the status AND escalated to an error
        // toast, because a status line nobody is looking at is not a
        // disclosure. The remedy travels with it.
        let mut degraded = AutoShadeApp::default().but_workspace({ let mut view = WorkspaceView::default(); *view.busy_mut() = true; view });
        degraded
            .services().tx
            .send(Msg::Segmented(Ok((
                "Subject".into(),
                PathBuf::from("mask-subject.png"),
                "U^2-Net (FALLBACK - BiRefNet did not run)".into(),
            ))))
            .unwrap();
        degraded.poll_workers(&ctx);
        assert!(
            degraded.workspace().status().contains("FALLBACK"),
            "the degraded run must say so where the good one said its model: {}",
            degraded.workspace().status()
        );
        let alarm = degraded
            .workspace().chrome().toasts
            .iter()
            .find(|t| matches!(t.kind, ToastKind::Error))
            .expect("a silent degradation is the bug this closes");
        assert!(
            alarm.text.contains("torchvision"),
            "the warning must carry the remedy, not just the bad news: {}",
            alarm.text
        );
        assert_ne!(
            ok.workspace().status(), degraded.workspace().status(),
            "two backends, two sentences — this is the whole point"
        );

        // A sidecar too old to print the line says nothing, and the landing
        // must not invent a model name for it.
        let mut silent = AutoShadeApp::default().but_workspace({ let mut view = WorkspaceView::default(); *view.busy_mut() = true; view });
        silent
            .services().tx
            .send(Msg::Segmented(Ok((
                "Sky".into(),
                PathBuf::from("mask-sky.png"),
                String::new(),
            ))))
            .unwrap();
        silent.poll_workers(&ctx);
        assert!(
            !silent.workspace().status().contains("drawn by") && !silent.workspace().status().contains("绘制"),
            "an empty label must add no clause at all: {}",
            silent.workspace().status()
        );
        assert!(
            !silent.workspace().chrome().toasts.iter().any(|t| matches!(t.kind, ToastKind::Error)),
            "unknown is not a fallback"
        );
    }

    /// The Adherence slider is INERT without a direction that guides, and the
    /// tier the dial picks is the one the develop's request carries.
    ///
    /// The old spelling of this test asserted that a default app has an empty
    /// direction and the shipped adherence default - two facts about
    /// `Default`, both true whether or not the slider is gated at all. It named
    /// the disabling and checked none of it.
    ///
    /// MUTATION: delete the `add_enabled_ui(has_direction, ...)` wrapper in
    /// panels/ai.rs and phase (2) fails.
    #[test]
    fn gui_adherence_slider_is_disabled_without_a_direction() {
        let ctx = egui::Context::default();
        crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
        let frame = |app: &mut AutoShadeApp| -> Option<bool> {
            app.seams().adherence_gate_enabled = None; // this frame's evidence only
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 900.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::SidePanel::left("controls").default_width(320.0).show(ctx, |ui| {
                        app.commands().ai_panel(ui);
                    });
                },
            );
            app.seams().adherence_gate_enabled
        };
        // (1) no direction: the dial exists but cannot be moved.
        let mut app = AutoShadeApp::default();
        assert!(app.workspace().ai().guidance.trim().is_empty(), "premise: the default app has no direction");
        assert_eq!(frame(&mut app), Some(false), "a blank direction must leave the slider inert");
        // (2) whitespace is still blank - the gate trims.
        app.workspace_mut().ai_mut().guidance = "   \n\t ".into();
        assert_eq!(frame(&mut app), Some(false), "whitespace is not a direction");
        // (3) a real direction under the library guide: still inert — the
        // direction is not sent (library OR direction, 2026-10-14).
        app.workspace_mut().ai_mut().guidance = "warmer, moodier".into();
        assert_eq!(app.aip().guide, autoshade::pipeline::Guide::Library, "premise: the default guide");
        assert_eq!(frame(&mut app), Some(false), "a direction the library overrides has no dial");
        // (4) guided by that direction: live.
        app.aip_mut().guide = autoshade::pipeline::Guide::Direction;
        assert_eq!(frame(&mut app), Some(true), "with a guiding direction the dial must be usable");
        // …and the DIAL reaches the request as a tier, which is the only thing
        // the value is for.
        app.aip_mut().direction_adherence = 0.1;
        let hint = autoshade::recipe::DirectionAdherence::new(app.aip().direction_adherence);
        assert_eq!(hint.tier(), autoshade::recipe::AdherenceTier::Hint);
        app.aip_mut().direction_adherence = 0.95;
        let brief = autoshade::recipe::DirectionAdherence::new(app.aip().direction_adherence);
        assert_eq!(brief.tier(), autoshade::recipe::AdherenceTier::Brief);
        assert_ne!(hint.tier(), brief.tier(), "the dial must be able to change the tier");
        // …and the tooltip NAMES the tiers, which is what a user needs to know
        // the dial is not a magnitude.
        let tip = tr(Lang::En, K_AI_HOW_CLOSELY_THE_AI_FOLLOWS_YOUR_DIRECTION_DISABL);
        for tier in ["Hint", "Direct", "Brief"] {
            assert!(tip.contains(tier), "the tooltip must name the {tier} tier: {tip}");
        }
        // The zh half is a translation of the SAME key, so a drifted pair
        // would fall back to English here rather than fail silently.
        let zh = tr(Lang::Zh, K_AI_HOW_CLOSELY_THE_AI_FOLLOWS_YOUR_DIRECTION_DISABL);
        assert!(zh.contains("遵循程度"), "the zh tooltip must exist: {zh}");
    }

    /// Library OR direction (user ruling 2026-10-14): the switch greys the
    /// side it did not choose — the Direction field under the library, the
    /// Style slider under the direction — survives a restart, and is what the
    /// develop's request carries.
    ///
    /// MUTATION: drop either `add_enabled_ui(by_direction / !by_direction, …)`
    /// wrapper in panels/ai.rs, or leave `guide` out of the saved prefs.
    #[test]
    fn gui_guide_switch_greys_the_other_side_and_persists() {
        use autoshade::pipeline::Guide;
        let ctx = egui::Context::default();
        crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
        let frame = |app: &mut AutoShadeApp| -> Option<(bool, bool)> {
            app.seams().guide_gate_enabled = None;
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 900.0))),
                    ..Default::default()
                },
                |ctx| {
                    egui::SidePanel::left("controls").default_width(320.0).show(ctx, |ui| {
                        app.commands().ai_panel(ui);
                    });
                },
            );
            app.seams().guide_gate_enabled
        };
        let mut app = AutoShadeApp::default();
        assert_eq!(frame(&mut app), Some((false, true)), "library: Direction greyed, Style live");
        app.aip_mut().guide = Guide::Direction;
        assert_eq!(frame(&mut app), Some((true, false)), "direction: Direction live, Style greyed");
        let seen = tall_frame(&mut app, |a, ui| a.commands().ai_panel(ui));
        assert!(seen.iter().any(|t| t.contains("Guided by the Direction")), "a greyed library must say why: {seen:?}");

        assert_eq!(Prefs::default().guide, Guide::Library, "an older prefs file decodes to the library");
        let json = serde_json::to_string(&Prefs { guide: Guide::Direction, ..Prefs::default() }).unwrap();
        let back: Prefs = serde_json::from_str(&json).unwrap();
        let mut restored = AutoShadeApp::default();
        restored.restore_prefs(back);
        assert_eq!(restored.aip().guide, Guide::Direction, "the switch must survive a restart");

        // The request half lives in the reflection layer, whose own tests drive
        // it (`logic::look::an_analyze_snapshots_the_panel_and_folds_a_region`
        // pins the direction/library rule and when typed words are sent); the
        // round trip above is the whole GUI-side claim.
    }

    /// S2: the phrase preference survives a restart, defaults OFF in BOTH
    /// defaults, and is what the build worker reads.
    ///
    /// OFF in both defaults is the load-bearing half. The pass downloads a
    /// 4.3 GB checkpoint and adds GPU minutes to every library build; a user
    /// who never asked for it must never pay for it, and a preferences file
    /// written before this key existed must decode to the same answer as a
    /// fresh install rather than to `true`.
    ///
    /// MUTATION THIS KILLS: default the key to `true` in either `Prefs` or
    /// `AutoShadeApp`, drop it from the saved preferences (the round trip then
    /// loses it), or wire the build worker to a literal switch instead of the
    /// preference.
    #[test]
    fn gui_describe_pref_round_trips() {
        assert!(
            !Prefs::default().style_describe,
            "a prefs file written before this key existed must decode to OFF"
        );
        assert!(
            !AutoShadeApp::default().aip().style_describe,
            "a fresh app must not start a 4.3 GB download"
        );
        // A pre-S2 preferences file really is such a file: it has no key at all.
        let old: Prefs = serde_json::from_str("{}").expect("an empty prefs file decodes");
        assert!(!old.style_describe, "a missing key must not read as ON");

        let prefs = Prefs { style_embed: true, style_describe: true, ..Prefs::default() };
        let json = serde_json::to_string(&prefs).expect("prefs serialize");
        assert!(json.contains("style_describe"), "the key must be written, not skipped: {json}");
        let decoded: Prefs = serde_json::from_str(&json).expect("prefs deserialize");
        assert!(decoded.style_describe, "the preference must survive a restart");

        // The preference is what the workers resolve, and the resolver answers
        // a VALUE — no test and no build writes the process environment.
        let on = AutoShadeApp::default().but_aip(AiPrefs {
                style_describe: true,
                ..Default::default()
            });
        let off = AutoShadeApp::default().but_aip(AiPrefs {
                style_describe: false,
                ..Default::default()
            });
        let unset = |_: &str| None;
        assert!(autoshade::library::DescribeSwitch::resolve_with(None, on.aip().style_describe, unset).on());
        assert!(!autoshade::library::DescribeSwitch::resolve_with(None, off.aip().style_describe, unset).on());

        // The preference crosses the file both ways: restore lands it on the
        // app, save writes the app's value back.
        let mut restored = AutoShadeApp::default();
        restored.restore_prefs(Prefs { style_describe: true, ..Prefs::default() });
        assert!(restored.aip().style_describe, "the saved preference must be restored at start-up");
        let storage = harness::PrefsStore::save(&mut restored);
        let saved = storage.prefs().unwrap();
        assert!(saved.style_describe, "…and written back when preferences are saved");
    }

    #[test]
    fn gui_prefs_round_trip_the_library_and_adherence_keys() {
        let prefs = Prefs {
            style_embed: true,
            style_src_dir: Some(std::env::temp_dir().join(format!("autoshade-looks-{}", std::process::id()))),
            direction_adherence: 0.91,
            ..Prefs::default()
        };
        let json = serde_json::to_string(&prefs).expect("prefs serialize");
        let decoded: Prefs = serde_json::from_str(&json).expect("prefs deserialize");
        assert!(decoded.style_embed);
        assert_eq!(decoded.style_src_dir, prefs.style_src_dir);
        // A prefs file from before the redesign still loads: its retired keys
        // are ignored, not refused.
        let older = json.replacen('{', r#"{"style_xmp_dir":"looks-x","looks_src_dir":"looks-y","use_looks":false,"#, 1);
        assert!(serde_json::from_str::<Prefs>(&older).is_ok(), "{older}");
        assert!((decoded.direction_adherence - 0.91).abs() < 1e-6);
    }

    /// C2 migration (user ruling 2026-08-31: migrate, do not accept the old
    /// name): the eframe prefs directory follows the rename. The core runs on
    /// explicit paths — the resolver half only names the two directories via
    /// `eframe::storage_dir` and is exercised by every real launch.
    ///
    /// The paths here are the ones production actually passes:
    /// `eframe::storage_dir(key)` is `<roaming>/<key>/data` on Windows, so the
    /// two directories are TWO levels deep and `<roaming>/AutoShade` does not
    /// exist on a machine that never ran the new name. An earlier version of
    /// this test renamed siblings under one existing base — a shape Windows
    /// never produces — and asserted that a missing destination parent SHOULD
    /// fall back, which pinned the shipped defect as correct behaviour.
    #[test]
    fn pre_rename_prefs_are_adopted_renamed_kept_or_fallen_back() {
        let base = std::env::temp_dir()
            .join(format!("autoshade-c2-prefs-adopt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // `eframe::storage_dir` shape: <base>/<key>/data, two levels deep.
        let storage_dir = |key: &str| base.join(key).join("data");
        let legacy = storage_dir("Autoshop");
        let current = storage_dir("AutoShade");
        // Nothing to adopt.
        assert_eq!(adopt_prefs_between(&current, &legacy, true), PrefsAdoption::Nothing);
        // The real thing: the directory moves wholesale, prefs file included —
        // and `base/AutoShade` is deliberately absent, exactly as on a machine
        // upgrading from a pre-rename build.
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("app.ron"), "(prefs)").unwrap();
        assert!(!base.join("AutoShade").exists(), "the destination parent is absent");
        assert_eq!(adopt_prefs_between(&current, &legacy, true), PrefsAdoption::Migrated);
        assert!(current.join("app.ron").is_file(), "the prefs came along");
        assert!(!legacy.exists(), "moved, not copied");
        // Both spellings present: the new one wins and the old is untouched.
        std::fs::create_dir_all(&legacy).unwrap();
        assert_eq!(adopt_prefs_between(&current, &legacy, true), PrefsAdoption::KeptBoth);
        assert!(legacy.exists(), "kept, not merged or deleted");
        // A rename that CANNOT be rescued by making the parent: a regular file
        // sits where the destination's parent directory would go, so
        // `create_dir_all` fails and the session keeps the legacy key rather
        // than resetting anyone's prefs.
        let blocked_parent = base.join("blocked");
        std::fs::write(&blocked_parent, "not a directory").unwrap();
        let blocked = blocked_parent.join("AutoShade").join("data");
        assert_eq!(adopt_prefs_between(&blocked, &legacy, true), PrefsAdoption::FellBack);
        assert!(legacy.exists(), "nothing lost on the failed arm");
        // The macOS opt-out arm: adopt=false answers Nothing even with a
        // legacy directory present.
        assert_eq!(adopt_prefs_between(&current, &legacy, false), PrefsAdoption::Nothing);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The key pair and the platform gate are one decision (the shape of
    /// store.rs's per-platform test): one spelling everywhere, adoption off
    /// only where `Application Support` could hold a stranger's directory.
    #[test]
    fn the_storage_key_and_its_adoption_are_one_decision_per_platform() {
        assert_eq!(
            (STORAGE_KEY, LEGACY_STORAGE_KEY, ADOPT_PRE_RENAME_PREFS),
            ("AutoShade", "Autoshop", !cfg!(target_os = "macos")),
            "the eframe key, the adopted spelling, and the macOS opt-out"
        );
    }

    /// The widest button rows, shared by the default-width and readable-width pins.
    fn app_with_every_button(lang: crate::i18n::Lang) -> AutoShadeApp {
        use autoshade::recipe::{ColourField, LocalAdjustment, MaskComponent, MaskGeometry};
        use crate::model::{Variant, VariantKind};
        let mk = |kind| Variant {
            kind,
            id: crate::model::new_variant_id(),
            name: None,
            recipe: EditRecipe::default(),
            base: None,
            origin: None,
            thumb: None,
        };
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {

                src_path: Some(std::env::temp_dir().join(format!("autoshade-library-{}/_buttons.ARW", std::process::id()))),

                ..Default::default()

            }; *editor.doc_mut() = Document {


                variants: vec![mk(VariantKind::Original), mk(VariantKind::Generated)],


                active: 1,


                ..Default::default()


            }; editor }).but_workspace({ let mut view = WorkspaceView::default(); *view.cards_mut() = CardsState {
                versions: vec![1],
                ..Default::default()
            }; *view.gal_mut() = GalleryState {
                multi_sel: [0usize].into_iter().collect(),
            
                ..Default::default()
            }; *view.fit_mut() = FitState {
                fit_ref: Some(std::env::temp_dir().join(format!("autoshade-library-{}/_buttons-reference.png", std::process::id()))),
                ..Default::default()
            }; view }).but_lang(lang);
        app.editor_mut().photo_mut().base_preview = Some(std::sync::Arc::new(image::DynamicImage::new_rgb8(64, 96)));
        app.editor_mut().doc_mut().recipe.masks.push(LocalAdjustment {
            mask: MaskGeometry::Bitmap { path: "mask-buttons.png".into() },
            components: vec![MaskComponent::default()],
            ..Default::default()
        });
        app.editor_mut().doc_mut().recipe.colour_field =
            Some(ColourField { x: 1, y: 1, b: 1, grid: vec![[0.0; 5]], amount: 0.5, enabled: true });
        // 2026-10-13: a lens blur, so its section's rows are laid too.
        app.editor_mut().doc_mut().recipe.lens_blur =
            Some(autoshade::render::LensBlur { depth_map: "depth-buttons.png".into(), ..Default::default() });
        // v1.5.0: a Point Color swatch — its row is a chip, a name and a
        // ✕, and its four sliders are laid beside the mixer's own.
        app.editor_mut().doc_mut().recipe.point_colors.push(autoshade::recipe::PointColor::sampled(0.9, 0.7, 0.5));
        // A histogram, so the readout's width can be pinned (2026-09-27).
        app.workspace_mut().chrome_mut().histogram = Some(vec![[0.5; 4]; 256]);
        app.workspace_mut().masks_mut().sel_mask = Some(0);
        app.commands().start_mask_brush(None);
        assert!(app.workspace().masks().mask_brush.is_some(), "{lang:?}: the brush session armed");
        app
    }

    /// The buttons' geometry, in one place: at the default panel widths a row
    /// stands exactly one line tall (glyph buttons square), no panel grows past
    /// its default, and every row-filling button shares the prompts' readable
    /// ceiling — the histogram readout included (user report 2026-09-27).
    #[test]
    fn every_button_keeps_its_row_and_the_readable_ceiling() {
        {

        use crate::buttons::DRAWN;
        for lang in [crate::i18n::Lang::En, crate::i18n::Lang::Zh] {
            let ctx = egui::Context::default();
            crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
            let row = ctx.style().spacing.interact_size.y;
            let mut app = app_with_every_button(lang);
            let input = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 20_000.0),
                )),
                ..Default::default()
            };
            let mut widths: Vec<[f32; 2]> = Vec::new();
            let mut rects: Vec<[egui::Rect; 2]> = Vec::new();
            let mut sections = [0.0f32; 3];
            for _ in 0..3 {
                DRAWN.with_borrow_mut(Vec::clear); // the LAST frame's registry only
                let _ = ctx.run(input(), |ctx| {
                    ctx.memory_mut(|m| m.set_everything_is_visible(true));
                    app.upd_top_bar(ctx);
                    let gallery = egui::SidePanel::left("gallery")
                        .default_width(240.0)
                        .show(ctx, |ui| app.draw_panel(|view| view.gallery_panel(ui)));
                    let controls = egui::SidePanel::left("controls").default_width(320.0).show(ctx, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            app.commands().ai_panel(ui);
                            sections[0] = ui.min_rect().width();
                            app.commands().develop_panel(ui);
                            sections[1] = ui.min_rect().width();
                            app.commands().retouch_panel(ui);
                            sections[2] = ui.min_rect().width();
                        });
                    });
                    egui::Window::new(tr(lang, K_APP_SETTINGS))
                        .id(egui::Id::new("settings_window"))
                        .default_width(480.0)
                        .show(ctx, |ui| app.draw_panel(|view| view.settings_ui(ui)));
                    widths.push([gallery.response.rect.width(), controls.response.rect.width()]);
                    rects.push([gallery.response.rect, controls.response.rect]);
                });
            }
            let drawn = DRAWN.with_borrow(|d| d.clone());
            for (i, (panel, default)) in [("gallery", 240.0f32), ("controls", 320.0)].into_iter().enumerate() {
                let seen: Vec<f32> = widths.iter().map(|w| w[i]).collect();
                let edge = rects[2][i].left() + default + 0.5;
                let past: Vec<String> = drawn
                    .iter()
                    .filter(|d| d.rect.left() >= rects[2][i].left() && d.rect.right() > edge)
                    .map(|d| format!("{} {:?} {:?}", d.kind, d.label, d.rect))
                    .collect();
                assert!(
                    seen.iter().all(|w| (w - default).abs() <= 0.5),
                    "{lang:?}: the {panel} panel left its {default} px default: {seen:?} — a row overflows it                      (section widths ai/develop/retouch {sections:?}; buttons past the edge: {past:?})"
                );
            }
            assert!(drawn.iter().any(|d| d.label == tr(lang, K_AI_ADJUST)),
                "{lang:?}: the new adjust button reached DRAWN");
            assert!(drawn.len() >= 60, "{lang:?}: only {} buttons reached the registry", drawn.len());
            for d in &drawn {
                assert!(
                    (d.rect.height() - row).abs() <= 0.5,
                    "{lang:?}: {} {:?} stands {:.1} px, not one row ({row})",
                    d.kind, d.label, d.rect.height()
                );
                if d.kind.starts_with("glyph") {
                    assert!(
                        (d.rect.width() - row).abs() <= 0.5,
                        "{lang:?}: {} {:?} is {:.1} px wide, not a square",
                        d.kind, d.label, d.rect.width()
                    );
                }
                assert!(d.fits, "{lang:?}: {} {:?} does not fit its cell on one line", d.kind, d.label);
            }
            assert!(
                drawn.iter().filter(|d| d.kind == "primary").count() >= 8,
                "{lang:?}: the primary verbs are on screen"
            );
        }
        }
        {

        use crate::buttons::DRAWN;
        for lang in [crate::i18n::Lang::En, crate::i18n::Lang::Zh] {
            for controls_width in [800.0, 1600.0] {
                let ctx = egui::Context::default();
                crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
                let mut app = app_with_every_button(lang);
                let mut widths = Vec::new();
                for frame in 0..3 {
                    DRAWN.with_borrow_mut(Vec::clear);
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(2400.0, 1200.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            ctx.memory_mut(|m| m.set_everything_is_visible(true));
                            let gallery = egui::SidePanel::left("gallery")
                                .default_width(800.0)
                                .show(ctx, |ui| app.draw_panel(|view| view.gallery_panel(ui)));
                            let controls = controls_panel(ctx, &mut app, controls_width);
                            widths.push([gallery.response.rect.width(), controls.width()]);
                        },
                    );
                    let drawn = DRAWN.with_borrow(|d| d.clone());
                    // The histogram readout used to be the one widget that grew
                    // with the panel without limit (user report 2026-09-27).
                    let hist = app.seams().histogram_rect.expect("the histogram readout was drawn");
                    assert!(
                        hist.width() <= crate::theme::FIELD_W_MAX + 0.5,
                        "{lang:?}, {controls_width} px, frame {frame}: the histogram is {:.1} px wide — past the {:.0} px readable ceiling",
                        hist.width(), crate::theme::FIELD_W_MAX
                    );
                    for label in [
                        tr(lang, K_RETOUCH_AI_HEAL_AUTO), tr(lang, K_RETOUCH_HEAL_AREA),
                        tr(lang, K_RETOUCH_ENTER_STAMP), tr(lang, K_RETOUCH_CLONE_AREA),
                        tr(lang, K_AI_REVERSE_FIT_RECIPE), tr(lang, K_AI_EXTRACT_STYLE),
                        tr(lang, K_GALLERY_COPY_RECIPE), tr(lang, K_DEVELOP_LINEAR__2), tr(lang, K_DEVELOP_ERASE),
                    ] {
                        assert!(
                            drawn.iter().any(|d| d.label == label),
                            "{lang:?}, {controls_width} px, frame {frame}: missing button {label:?}"
                        );
                    }
                    for d in &drawn {
                        assert!(
                            d.rect.width() <= crate::theme::FIELD_W_MAX + 0.5,
                            "{lang:?}, {controls_width} px, frame {frame}: {} {:?} is {:.1} px wide — \
                             past the {:.0} px readable ceiling",
                            d.kind, d.label, d.rect.width(), crate::theme::FIELD_W_MAX
                        );
                        assert!(
                            d.fits,
                            "{lang:?}, {controls_width} px, frame {frame}: {} {:?} does not fit its cell on one line",
                            d.kind, d.label
                        );
                    }
                }
                // The controls panel stops at its cap (2026-10-13): a drag
                // past it leaves the panel at `controls_w_max`.
                let controls_expected = controls_width.min(crate::theme::controls_w_max(&ctx.style()));
                for (i, (panel, expected)) in [("gallery", 800.0), ("controls", controls_expected)].into_iter().enumerate() {
                    let seen: Vec<f32> = widths.iter().map(|w| w[i]).collect();
                    assert!(
                        (seen[0] - seen[2]).abs() < 0.5,
                        "{lang:?}: the wide {panel} panel must not grow across frames: {seen:?}"
                    );
                    assert!(
                        seen.iter().all(|w| (w - expected).abs() <= 0.5),
                        "{lang:?}: the {panel} panel left its {expected} px width: {seen:?}"
                    );
                }
            }
        }
        }
    }

    /// R27: the clone stamp is a pixel worker like heal and fill — its mask
    /// is exported from the turned canvas while `retouch::clone_stamp`
    /// re-develops the un-turned frame — so it takes the same refusal gate,
    /// before any output claim or worker. MUTATION: drop the
    /// `|| self.refuse_pixel_work_on_a_turned_photo()` in start_clone and the
    /// status below becomes the Alt+click prompt.
    #[test]
    fn a_clone_on_a_turned_photo_is_refused_before_it_asks_for_a_source_point() {
        for lang in [Lang::En, Lang::Zh] {
            let mut app = AutoShadeApp::default().but_lang(lang);
            app.editor_mut().photo_mut().src_path = Some(PathBuf::from("clone-turn-guard.png"));
            app.workspace_mut().tools_mut().clone_src = None; // the gate answers before this is even asked for
            let refusal = tr(
                lang,
                K_RETOUCH_THIS_PHOTO_IS_TURNED_AND_PIXEL_REPAIRS_STILL_WOR,
            );
            let ask = tr(lang, K_ACTIONS_ALT_CLICK_TO_SET_THE_CLONE_SOURCE_FIRST);
            for turns in [1u8, 2, 3, 5] {
                app.editor_mut().doc_mut().recipe.quarter_turns = turns;
                app.workspace_mut().status_mut().clear();
                app.commands().start_clone();
                assert!(!app.workspace().busy(), "quarter_turns={turns}: a refused clone never reaches a worker");
                assert_eq!(app.workspace().status(), refusal, "quarter_turns={turns}");
            }
            // No turn, or a full one, IS the un-turned frame: the verb goes on
            // to its own next check, exactly as heal and fill do.
            for turns in [0u8, 4] {
                app.editor_mut().doc_mut().recipe.quarter_turns = turns;
                app.workspace_mut().status_mut().clear();
                app.commands().start_clone();
                assert!(!app.workspace().busy());
                assert_eq!(app.workspace().status(), ask, "quarter_turns={turns}");
            }
        }
    }

    /// The AI mask refine is a pixel worker too (its guide is decoded in the
    /// EXIF frame, the raster it refines lives in the turned plate frame), so
    /// it takes the same refusal gate before any worker. MUTATION: drop the
    /// `|| self.refuse_pixel_work_on_a_turned_photo()` in start_mask_refine.
    #[test]
    fn a_mask_refine_on_a_turned_photo_is_refused_before_it_spawns() {
        for lang in [Lang::En, Lang::Zh] {
            let mut app = AutoShadeApp::default().but_lang(lang);
            app.editor_mut().photo_mut().src_path = Some(PathBuf::from("refine-turn-guard.png"));
            let refusal = tr(
                lang,
                K_RETOUCH_THIS_PHOTO_IS_TURNED_AND_PIXEL_REPAIRS_STILL_WOR,
            );
            for turns in [1u8, 2, 3] {
                app.editor_mut().doc_mut().recipe.quarter_turns = turns;
                app.workspace_mut().status_mut().clear();
                app.commands().start_mask_refine(0);
                assert!(!app.workspace().busy(), "quarter_turns={turns}: a refused refine never reaches a worker");
                assert_eq!(app.workspace().status(), refusal, "quarter_turns={turns}");
            }
            // Un-turned, with no mask at that index: the verb returns quietly.
            app.editor_mut().doc_mut().recipe.quarter_turns = 0;
            app.workspace_mut().status_mut().clear();
            app.commands().start_mask_refine(0);
            assert!(!app.workspace().busy());
            assert!(app.workspace().status().is_empty(), "{}", app.workspace().status());
        }
    }

    /// An unreadable CENTRAL XMP is `Unreadable`, exactly as an unreadable
    /// recipe.json is — not `NoopOnly`. The XMP loop set `any` and broke
    /// without recording the error, so the store answered "sidecars exist,
    /// nothing in them": the open note said nothing and the next Ctrl+S
    /// overwrote the file with no version backup. MUTATION: drop the
    /// `parse_err` assignment in the XMP loop's read-error arm and this
    /// sees NoopOnly again.
    #[test]
    fn an_unreadable_central_xmp_is_reported_not_folded_into_noop() {
        let src = &std::env::temp_dir().join(format!("autoshade-library-{}/_xmp_unreadable_test.ARW", std::process::id())); // never touched
        let dev = autoshade::store::develop_dir(src);
        let _ = std::fs::remove_dir_all(&dev); // a crashed earlier run may have left files
        std::fs::create_dir_all(&dev).unwrap();
        let xp = autoshade::pipeline::xmp_target(src);
        let _scrub = Scrub(vec![dev.clone(), xp.clone()]);
        // Present but unreadable: `read_text_capped` answers InvalidData (not
        // NotFound) for bytes that are not UTF-8 — the same door an over-cap
        // or permission-denied file comes through.
        std::fs::write(&xp, [0xffu8, 0xfe, 0x00, 0x80]).unwrap();
        let RestoredDevelop { saved, .. } = read_saved_develop(src);
        let SavedDevelop::Unreadable { err, fallback } = saved else {
            panic!("an unreadable central XMP must be reported, not skipped");
        };
        assert!(fallback.is_none(), "nothing else answered");
        assert!(err.starts_with("read "), "the recipe loop's own phrasing: {err}");
    }

    /// A strip entry's recipe is untrusted input like the active card's —
    /// `read_saved_develop_locked` clamps recipe.json on the way in, but
    /// `strip_from_record` copied variants.json recipes verbatim, and a card
    /// is one click from BEING the canvas: a crop wider than the frame
    /// reached the crop-nudge maths in `upd_shortcuts`
    /// (`x.clamp(0.0, 1.0 - w)` with `w > 1`), which panics on `min > max`.
    /// MUTATION: drop the `recipe.clamp()` in strip_from_record.
    #[test]
    fn strip_from_record_clamps_each_restored_recipe() {
        let wild = EditRecipe {
            crop: Some(autoshade::recipe::Crop { left: 0.0, top: 0.0, right: 2.0, bottom: 1.5 }),
            contrast: 9_000.0,
            ..Default::default()
        };
        let rec = autoshade::store::VariantsRecord {
            extra: Default::default(),
            v: 1,
            active_kind: "original".into(),
            active_pos: 0,
            active_id: None,
            active_name: None,
            others: vec![autoshade::store::VariantEntry {
                extra: Default::default(),
                kind: "fitted".into(),
                recipe: wild.clone(),
                origin: None,
                id: Some("card-wild".into()),
                name: None,
            }],
        };
        let (strip, _) = crate::persist::strip_from_record(&rec, None);
        let mut expected = wild;
        let _ = expected.clamp();
        assert_eq!(strip[0].recipe, expected, "the restored card carries the CLAMPED recipe");
        let c = strip[0].recipe.crop.expect("a clamped crop is kept: ordered and in-frame");
        let w = c.right - c.left;
        assert!(w <= 1.0, "crop width {w} would panic the keyboard nudge's clamp(0.0, 1.0 - w)");
        // The nudge maths itself, as upd_shortcuts spells it — a panic here is the bug.
        let _ = (c.left + 0.005).clamp(0.0, 1.0 - w);
    }

    /// egui keys a window's remembered state (position) off its id, and the
    /// id defaults to the TITLE — every window title here is localised, so a
    /// language switch dropped the Shortcuts window's state while Settings,
    /// with a fixed id, kept its own. Pinned in the source: a headless test
    /// cannot drive eframe's window memory. Each builder chain is located by
    /// its constructor, never by its title text. MUTATION: delete the

    /// R27, every door: each pixel worker that lands a BAKED master takes
    /// the turned-photo gate before any output claim or worker thread — the
    /// masked ones because their mask is in the turned frame, and the
    /// mask-free ones (AI denoise, stack, reimagine) because their master
    /// lands as a ◈/▦/✨ card whose raster is in the EXIF frame: `load_active`
    /// installs it as the plate and `sync_base_turns` cannot turn a baked
    /// card. MUTATION: drop the gate at any one of the four doors below and
    /// that door's status is no longer the refusal.
    #[test]
    fn every_pixel_worker_refuses_a_turned_photo() {
        let (mut app, _scrub) = app_with_masked_photo("turned-doors");
        let src = std::env::temp_dir().join(format!("autoshade-turned-doors-{}.arw", std::process::id()));
        std::fs::write(&src, b"raw").unwrap();
        let _src_scrub = Scrub(vec![src.clone()]);
        app.editor_mut().photo_mut().src_path = Some(src.clone());
        app.editor_mut().doc_mut().recipe.quarter_turns = 1;
        type Door = fn(&mut AutoShadeApp, PathBuf);
        let doors: [(&str, Door); 4] = [
            ("start_ai_denoise", |a, _| a.commands().start_ai_denoise()),
            ("start_clone", |a, _| a.commands().start_clone()),
            ("start_reimagine", |a, _| a.commands().start_reimagine()),
            ("start_stack", |a, p| a.commands().start_stack(vec![p])),
        ];
        for (name, door) in doors {
            app.workspace_mut().set_busy(false);
            app.workspace_mut().status_mut().clear();
            door(&mut app, src.clone());
            assert!(!app.workspace().busy(), "{name}: a refused pixel worker never reaches a worker thread");
            assert!(app.workspace().status().contains("this photo is turned"), "{name}: {}", app.workspace().status());
        }
    }

    /// A turn moves the plate AND the paint canvas: the brush canvas is sized
    /// to the plate, so after `sync_base_turns` transposes the plate a canvas
    /// left in the old frame painted sideways (and off the long edge). The
    /// canvas is REBOUND — empty — to the plate's new frame. MUTATION: drop
    /// the `rebind_paint_canvas` call in sync_base_turns.
    #[test]
    fn a_turn_rebinds_the_paint_canvas_to_the_transposed_plate() {
        let (mut app, _scrub) = app_with_masked_photo("rotate-paint");
        let ctx = egui::Context::default();
        let plate = std::sync::Arc::new(image::DynamicImage::new_rgb8(8, 6));
        app.editor_mut().photo_mut().base_preview = Some(plate.clone());
        app.editor_mut().photo_mut().source_preview = Some(plate);
        app.editor_mut().photo_mut().base_turns = 0;
        app.workspace_mut().masks_mut().mask_paint = Some(image::RgbaImage::new(8, 6));
        app.editor_mut().doc_mut().recipe.quarter_turns = 1;
        { let (e, w, l) = app.editor_workspace_mut(); e.sync_base_turns(w, l, &ctx) };
        assert_eq!(app.editor().photo().base_preview.as_ref().unwrap().dimensions(), (6, 8));
        assert_eq!(
            app.workspace().masks().mask_paint.as_ref().unwrap().dimensions(),
            (6, 8),
            "the paint canvas follows the plate's frame"
        );
        assert!(!app.workspace().has_painted_mask(), "a rebound canvas starts empty");
    }

    /// The coverage overlay and the range eyedropper share the one-slot
    /// `overlay_ref` cache, compared by recipe equality, so both must build
    /// the reference recipe through ONE builder — the hand-copied twin that
    /// forgot the manual CA pair missed the cache on every click under a
    /// non-zero Red/cyan or Blue/yellow and paid a full develop on the UI
    /// thread. MUTATION: drop `pre.ca_r = 0.0` from range_reference_recipe,
    /// or rebuild the copy by hand in either caller.
    #[test]
    fn the_range_reference_is_one_builder_with_every_geometry_field_stripped() {
        let (mut app, _scrub) = app_with_masked_photo("range-ref");
        let first = app.editor().doc().recipe.masks[0].clone();
        app.editor_mut().doc_mut().recipe.masks.push(first);
        app.editor_mut().doc_mut().recipe.ca_r = 0.4;
        app.editor_mut().doc_mut().recipe.ca_b = -0.3;
        app.editor_mut().doc_mut().recipe.straighten_deg = 2.0;
        app.editor_mut().doc_mut().recipe.lens_distortion = 0.1;
        let pre = app.editor().range_reference_recipe(1);
        assert_eq!(pre.masks.len(), 1, "mask 1's reference is the prefix before it");
        assert_eq!(
            (pre.ca_r, pre.ca_b),
            (0.0, 0.0),
            "manual CA is geometry — stripped, or the eyedropper misses the overlay's cache"
        );
        assert_eq!((pre.straighten_deg, pre.lens_distortion), (0.0, 0.0));
        assert!(pre.crop.is_none());
    }

    /// 2026-09-27 (user report 「UI 按钮的大小参差不齐」, decided on a probe of
    /// every drawn button): in the side panels every text verb is laid in a
    /// grid cell — a row's one verb fills the row, two or three share it, a
    /// verb beside a checkbox or a label takes one cell of the two-column
    /// grid — so no verb is sized by its own label. Two list/header verbs are
    /// the exceptions (a version row's 「Load」, the gallery's 「🗂 Open
    /// folder…」 beside its heading); the toolbar, the dialogs and Settings
    /// keep own-width verbs. Also pinned: the AI section's rows start on the
    /// same x as the Develop and Retouch rows (its outer fold used to indent
    /// them 18 px further), and each brush fold carries its own
    /// 「🖌 Paint area」 / 「Clear area」 pair.
    #[test]
    fn every_side_panel_text_verb_is_laid_in_a_cell_and_the_sections_align() {
        use crate::buttons::DRAWN;
        for lang in [crate::i18n::Lang::En, crate::i18n::Lang::Zh] {
            let ctx = egui::Context::default();
            crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
            let mut app = app_with_every_button(lang);
            let input = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 20_000.0))),
                ..Default::default()
            };
            let mut rects = [egui::Rect::NOTHING; 2];
            let mut sections: Vec<(&'static str, f32, f32)> = Vec::new();
            for _ in 0..3 {
                DRAWN.with_borrow_mut(Vec::clear);
                let mut inner = Vec::new();
                let _ = ctx.run(input(), |ctx| {
                    ctx.memory_mut(|m| m.set_everything_is_visible(true));
                    app.upd_top_bar(ctx);
                    let gallery = egui::SidePanel::left("gallery")
                        .default_width(240.0)
                        .show(ctx, |ui| app.draw_panel(|view| view.gallery_panel(ui)));
                    let controls = egui::SidePanel::left("controls").default_width(320.0).show(ctx, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            let y0 = ui.min_rect().top();
                            app.commands().ai_panel(ui);
                            let y1 = ui.min_rect().bottom();
                            app.commands().develop_panel(ui);
                            let y2 = ui.min_rect().bottom();
                            app.commands().retouch_panel(ui);
                            let y3 = ui.min_rect().bottom();
                            inner = vec![("ai", y0, y1), ("develop", y1, y2), ("retouch", y2, y3)];
                        });
                    });
                    rects = [gallery.response.rect, controls.response.rect];
                });
                sections = inner;
            }
            let drawn = DRAWN.with_borrow(|d| d.clone());
            let text = |d: &crate::buttons::Drawn| !d.kind.starts_with("glyph");
            let load = tr(lang, K_DEVELOP_LOAD);
            let open = tr(lang, K_APP_OPEN_FOLDER);
            for d in drawn.iter().filter(|d| text(d)) {
                let c = d.rect.center();
                let (panel, exempt) = if rects[1].contains(c) {
                    ("controls", d.label == load)
                } else if rects[0].contains(c) {
                    ("gallery", d.label == open)
                } else {
                    continue;
                };
                assert!(
                    d.cell || exempt,
                    "{lang:?}: {} {:?} in the {panel} panel is laid at its own width ({:.0} px), not in a cell",
                    d.kind, d.label, d.rect.width()
                );
            }
            let left_of = |name: &str| -> f32 {
                let (_, y0, y1) = sections.iter().find(|s| s.0 == name).copied().expect("the section was laid");
                drawn
                    .iter()
                    .filter(|d| text(d) && rects[1].contains(d.rect.center()))
                    .filter(|d| d.rect.center().y >= y0 && d.rect.center().y < y1)
                    .map(|d| d.rect.left())
                    .fold(f32::INFINITY, f32::min)
            };
            let (ai, dev, ret) = (left_of("ai"), left_of("develop"), left_of("retouch"));
            assert!(ai.is_finite() && dev.is_finite() && ret.is_finite(), "{lang:?}: every section drew a text verb");
            assert!(
                (ai - dev).abs() <= 0.5 && (dev - ret).abs() <= 0.5,
                "{lang:?}: the sections' rows start at different x: ai {ai}, develop {dev}, retouch {ret}"
            );
            let count = |label: &str| drawn.iter().filter(|d| d.label == label).count();
            assert_eq!(count(tr(lang, K_RETOUCH_PAINT_AREA)), 3, "{lang:?}: Fill, Heal and Adjust each carry their own brush toggle");
            assert_eq!(count(tr(lang, K_RETOUCH_CLEAR_AREA)), 4, "{lang:?}: Fill, Heal, the Stamp and Adjust each carry their own clear verb");
        }
    }

    /// Each tool keeps its own painted area (user decision 2026-09-27): what
    /// is painted for Heal is not what Fill, the Stamp or Adjust read; a mask
    /// brush session stashes the live area and gives it back; a plate rebind
    /// starts every tool blank; a landing clears only the area its result
    /// consumed.
    #[test]
    fn each_tool_keeps_its_own_painted_area() {
        let paint = |app: &mut AutoShadeApp| {
            app.workspace_mut().masks_mut().mask_paint.as_mut().unwrap().put_pixel(1, 1, image::Rgba([255, 64, 64, 160]));
            app.workspace_mut().paint_mask_changed(Some(true));
        };
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {

                base_preview: Some(std::sync::Arc::new(image::DynamicImage::new_rgb8(8, 6))),

                ..Default::default()

            }; editor });
        let __lang = app.lang();
        app.workspace_mut().rebind_paint_canvas(__lang, 8, 6);
        assert_eq!(app.workspace().masks().paint_owner, BrushOwner::Fill, "Fill's area is the canvas by default");

        app.commands().arm_brush(BrushOwner::Heal);
        assert!(app.workspace().masks().paint_mode && app.workspace().brush_armed(BrushOwner::Heal) && !app.workspace().brush_armed(BrushOwner::Fill));
        paint(&mut app);
        assert!(app.workspace().area_painted(BrushOwner::Heal));
        assert!(!app.workspace().area_painted(BrushOwner::Fill) && !app.workspace().area_painted(BrushOwner::Adjust));

        // Fill's turn: a blank canvas, and Heal's strokes wait in the store
        // with their memo — reading them back scans nothing.
        let scans = app.workspace().overlay().mask_presence_scans.get();
        app.commands().arm_brush(BrushOwner::Fill);
        assert!(!app.workspace().has_painted_mask(), "Fill sees its own blank area, not Heal's strokes");
        assert!(app.workspace().area_painted(BrushOwner::Heal), "Heal's strokes survive off the canvas");
        assert_eq!(app.workspace().overlay().mask_presence_scans.get(), scans, "a stored memo answers without a scan");
        assert!(app.workspace().export_area_png(BrushOwner::Heal).is_some());
        assert!(app.workspace().export_area_png(BrushOwner::Fill).is_none());

        // The Stamp arms through clone_mode and takes the paint flag down.
        app.commands().arm_brush(BrushOwner::Stamp);
        assert!(app.workspace().tools().clone_mode && !app.workspace().masks().paint_mode && app.workspace().brush_armed(BrushOwner::Stamp));
        paint(&mut app);
        assert!(app.workspace().area_painted(BrushOwner::Stamp) && app.workspace().area_painted(BrushOwner::Heal));

        // Clearing one tool's area leaves the others'.
        app.workspace_mut().clear_area(BrushOwner::Heal);
        assert!(!app.workspace().area_painted(BrushOwner::Heal) && app.workspace().area_painted(BrushOwner::Stamp));
        app.commands().arm_brush(BrushOwner::Heal);
        assert!(!app.workspace().has_painted_mask(), "Heal comes back blank");

        // A mask-brush session stashes the live area and gives it back.
        app.commands().arm_brush(BrushOwner::Adjust);
        paint(&mut app);
        app.commands().start_mask_brush(None);
        assert!(!app.workspace().has_painted_mask(), "the session starts on its own blank canvas");
        assert!(app.workspace().area_painted(BrushOwner::Adjust), "Adjust's strokes wait in the store");
        assert!(!app.workspace().brush_armed(BrushOwner::Adjust), "a session belongs to no tool");
        app.workspace_mut().disarm_brush();
        assert!(app.workspace().masks().mask_brush.is_none() && !app.workspace().masks().paint_mode);
        assert!(
            app.workspace().has_painted_mask() && app.workspace().masks().paint_owner == BrushOwner::Adjust,
            "the session's end restores Adjust's area"
        );

        // The landing clears only the area its result consumed.
        assert_eq!(RetouchNote::Filled(PathBuf::from("x.png")).consumed_area(), Some(BrushOwner::Fill));
        assert_eq!(RetouchNote::Cloned { n: 1, out: PathBuf::from("x.png") }.consumed_area(), Some(BrushOwner::Stamp));
        assert_eq!(
            RetouchNote::Reimagined { out: PathBuf::from("x.png"), divergence: 0.1, discarded: None }.consumed_area(),
            None
        );

        // A plate rebind starts every tool blank.
        let __lang = app.lang();
        app.workspace_mut().rebind_paint_canvas(__lang, 8, 6);
        assert!(!app.workspace().area_painted(BrushOwner::Adjust) && !app.workspace().area_painted(BrushOwner::Stamp));
        assert!(app.workspace().masks().paint_store.iter().all(Option::is_none));
    }
