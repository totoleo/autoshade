// One part of the GUI's tests (src/bin/gui/tests.rs includes it): the
// Portraits panel in the one workspace's sidebar (panels/portrait.rs,
// design-portraits-mode.md §3–§4; v1.14.0).

    /// The four masks the find step makes, over rasters that need not exist
    /// (the panel reads the recipe, not the pixels).
    fn found_masks() -> Vec<autoshade::recipe::LocalAdjustment> {
        vec![
            autoshade::face::portrait_adjustment(autoshade::recipe::MaskRole::Face, "mask-face.png".into()),
            autoshade::face::portrait_adjustment(autoshade::recipe::MaskRole::Eyes, "mask-eyes.png".into()),
            autoshade::face::skin_adjustment("mask-face.png".into(), [0.8, 0.6, 0.5], [0.4, 0.3]),
            autoshade::face::background_adjustment("mask-subject.png".into()),
        ]
    }

    /// The photo with every button armed, with or without the people found.
    fn portrait_app(lang: Lang, found: bool) -> AutoShadeApp {
        let mut app = app_with_every_button(lang);
        if found {
            app.editor_mut().doc_mut().recipe.masks.extend(found_masks());
        }
        app
    }

    /// Every text the Portraits panel draws on its own, in one tall frame.
    fn portrait_panel_texts(app: &mut AutoShadeApp) -> Vec<String> {
        let mut ctx = harness::Ctx::new();
        crate::theme::install_fonts(&ctx);
        crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
        let mut out = None;
        for i in 0..3 {
            out = Some(ctx.run_with(420.0, 4000.0, Some(i as f64), |ctx| {
                harness::central_panel(ctx, |ui| app.commands().portrait_panel(ui));
            }));
        }
        out.unwrap().texts()
    }

    /// Design §3 on one frame, in both languages: the panel's heading and its
    /// four groups with every control the sketch names, and none of the
    /// workspace's trade words — the panel speaks to people who photograph
    /// people, even though the full workspace stays below it.
    ///
    /// MUTATION THIS CATCHES: drop a group or a control from `portrait_panel`,
    /// or word one of them with a trade term (a "mask", a "develop"), and a
    /// line below fails.
    #[test]
    fn the_portrait_panel_draws_four_groups_in_plain_words_in_either_language() {
        let words = ["配方", "显影", "反推", "变体", "蒙版", "API", "recipe", "develop", "reverse-fit", "variant", "mask"];
        for lang in [Lang::En, Lang::Zh] {
            let mut app = portrait_app(lang, true);
            let texts = portrait_panel_texts(&mut app);
            let has = |s: &str| texts.iter().any(|t| t.starts_with(s));
            // Through a function pointer: the keys below are data here, and
            // the i18n audit reads a `tr` call's argument as a call site.
            let say: fn(Lang, &'static str) -> &'static str = tr;
            for key in [
                "Portraits",
                "People and background",
                "🤖 Find people and faces",
                "✨ AI portrait look",
                "Look strength",
                "Skin",
                "Soften skin",
                "Skin warmth",
                "Skin colour intensity",
                "Keep skin natural",
                "🤖 Remove blemishes",
                "Eyes",
                "Brighten eyes",
                "Crisp eyes",
                "Background",
                "Blur amount",
                "Sharp range",
                "Keep the person sharp",
                "Darken background",
                "Fade background colour",
            ] {
                assert!(has(say(lang, key)), "{lang:?}: the panel draws {key:?}");
            }
            assert!(!has(tr(lang, K_PORTRAIT_NO_FACE_FOUND_IN_THIS_PHOTO)), "{lang:?}: faces are found, nothing is greyed");
            for text in &texts {
                let low = text.to_lowercase();
                for w in words {
                    let hit = if w.is_ascii() && w != "API" { low.contains(w) } else { text.contains(w) };
                    assert!(!hit, "{lang:?}: {text:?} says {w:?}");
                }
            }
            // The hovers are drawn only under the pointer: read them from the
            // source, where every one is a literal handed to the translator.
            let body = gui_function("portrait_panel");
            let call = format!("{}(lang, \"", "tr");
            // The translator takes keys as `&'static str`; the literals are
            // small and this is a test, so leak them.
            let lits: Vec<&'static str> = body
                .split(call.as_str())
                .skip(1)
                .filter_map(|s| s.split('"').next())
                .map(|s| Box::leak(s.to_string().into_boxed_str()) as &'static str)
                .collect();
            for lit in &lits {
                let shown = say(lang, lit);
                for w in words {
                    let hit = if w.is_ascii() && w != "API" { shown.to_lowercase().contains(w) } else { shown.contains(w) };
                    assert!(!hit, "{lang:?}: {shown:?} says {w:?}");
                }
            }
        }
    }

    /// The panel sits at the TOP of the one sidebar, above Normal's groups
    /// and the full workspace below.
    ///
    /// MUTATION THIS CATCHES: the panel drawn after Normal's groups or below
    /// the AI panel in `controls_column`.
    #[test]
    fn the_portrait_panel_tops_the_sidebar() {
        let mut app = app_with_every_button(Lang::En);
        let mut ctx = harness::Ctx::new();
        crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
        let mut out = None;
        for i in 0..3 {
            out = Some(ctx.run_with(1400.0, 20_000.0, Some(i as f64), |ctx| {
                controls_panel(ctx, &mut app, 320.0);
            }));
        }
        let texts = out.unwrap().texts();
        let at = |key: &str| texts.iter().position(|t| t.starts_with(key));
        let panel = at("People and background").expect("the panel is drawn");
        for below in ["One-click look", "AI generation & reverse-fit", "Tone & WB"] {
            assert!(panel < at(below).expect("the panels below stay"), "{below:?} is drawn after the Portraits panel");
        }
    }

    /// No face found: the skin and eyes groups say so and are closed, the
    /// background group still works with the person alone; before any search
    /// they ask for one instead.
    #[test]
    fn without_a_face_the_skin_and_eyes_groups_are_greyed_and_say_why() {
        for lang in [Lang::En, Lang::Zh] {
            let mut app = portrait_app(lang, false);
            let photo = app.editor().photo().src_path.clone().unwrap();
            assert_eq!(app.editor().portrait_face_gate(&app.workspace(), lang), Some(tr(lang, K_PORTRAIT_FIND_THE_PEOPLE_AND_FACES_FIRST_THE_FIRST_GROUP)));
            // A search that found the person and no face.
            app.workspace_mut().portrait_mut().found = Some((photo, 0));
            app.editor_mut().doc_mut().recipe.masks.push(autoshade::face::background_adjustment("mask-subject.png".into()));
            let no_face = tr(lang, K_PORTRAIT_NO_FACE_FOUND_IN_THIS_PHOTO);
            assert_eq!(app.editor().portrait_face_gate(&app.workspace(), lang), Some(no_face));
            let texts = portrait_panel_texts(&mut app);
            assert_eq!(texts.iter().filter(|t| *t == no_face).count(), 2, "{lang:?}: both groups say it: {texts:?}");
            assert!(
                !texts.iter().any(|t| t == tr(lang, K_PORTRAIT_FIND_THE_PEOPLE_AND_FACES_FIRST_THE_FIRST_GROUP)),
                "{lang:?}: the background group works with the person alone"
            );
            assert_eq!(autoshade_subject(&app), Some("mask-subject.png".into()));
            // With a face, the gate opens.
            app.editor_mut().doc_mut().recipe.masks.extend(found_masks());
            assert_eq!(app.editor().portrait_face_gate(&app.workspace(), lang), None);
        }
    }

    fn autoshade_subject(app: &AutoShadeApp) -> Option<String> {
        autoshade::face::subject_raster(&app.editor().doc().recipe)
    }

    /// Keep the person sharp points the blur at the background mask's own
    /// raster (the subject matte) and off is no protection; nothing to protect
    /// before the person is found. The slider→field mappings this test used to
    /// pin live in `autoshade::face` now.
    ///
    /// MUTATION THIS CATCHES: `set_keep_person_sharp` protecting with another
    /// raster, or claiming protection with no subject.
    #[test]
    fn every_portrait_control_writes_its_field_on_its_role() {
        // The slider→field mappings are the library's (`autoshade::face`'s
        // own tests); what is the PANEL's is the keep-the-person-sharp switch.
        let mut app = portrait_app(Lang::En, true);
        // Keep the person sharp: the blur protects the background mask's own
        // raster — the subject matte — and off is no protection.
        assert!(app.editor_mut().set_keep_person_sharp(true));
        assert_eq!(app.editor().doc().recipe.lens_blur.as_ref().unwrap().protect.as_deref(), Some("mask-subject.png"));
        assert!(app.editor_mut().set_keep_person_sharp(false));
        assert_eq!(app.editor().doc().recipe.lens_blur.as_ref().unwrap().protect, None);
        let mut alone = portrait_app(Lang::En, false);
        assert!(!alone.editor_mut().set_keep_person_sharp(true), "nothing to protect with before the person is found");
        assert_eq!(alone.editor().doc().recipe.lens_blur.as_ref().unwrap().protect, None);
    }

    /// The find step's landing: the four masks with their roles; a second
    /// search re-points them and keeps their sliders; no face is said as
    /// such; a landing for another photo is dropped.
    #[test]
    fn finding_lands_four_role_masks_and_refreshes_them_in_place() {
        use crate::logic::portrait::{FoundFaces, PortraitFind};
        use crate::panels::portrait::PortraitLine;
        let mut app = portrait_app(Lang::En, false);
        let photo = app.editor().photo().src_path.clone().unwrap();
        let before = app.editor().doc().recipe.masks.len();
        let find = |tag: &str, n: usize| PortraitFind {
            photo: photo.clone(),
            subject: Ok(PathBuf::from(format!("mask-subject-{tag}.png"))),
            faces: Ok(Some(FoundFaces {
                n,
                face: PathBuf::from(format!("mask-face-{tag}.png")),
                eyes: PathBuf::from(format!("mask-eyes-{tag}.png")),
                seed: Some(([0.8, 0.6, 0.5], [0.4, 0.3])),
            })),
        };
        app.workspace_mut().set_busy(true);
        { let (e, w, _l) = app.editor_workspace_mut(); e.on_portrait_found(w, Lang::En, find("a", 2)) };
        assert!(!app.workspace().busy());
        let roles: Vec<autoshade::recipe::MaskRole> = app.editor().doc().recipe.masks[before..].iter().map(|m| m.role).collect();
        assert_eq!(roles, [autoshade::recipe::MaskRole::Face, autoshade::recipe::MaskRole::Eyes, autoshade::recipe::MaskRole::Skin, autoshade::recipe::MaskRole::Background]);
        let bg = &app.editor().doc().recipe.masks[before + 3];
        assert!(bg.inverted, "the background is the person, inverted");
        assert!(app.editor().doc().recipe.masks[before + 2].range.is_some(), "skin = the face narrowed by colour");
        assert_eq!(app.workspace().status(), "Found 2 faces");
        assert_eq!(app.workspace().portrait().found, Some((photo.clone(), 2)));
        // The user softens; a second search keeps that and moves the raster.
        autoshade::face::set_soften(&mut app.editor_mut().doc_mut().recipe.masks[before + 2], 30.0);
        { let (e, w, _l) = app.editor_workspace_mut(); e.on_portrait_found(w, Lang::En, find("b", 1)) };
        assert_eq!(app.editor().doc().recipe.masks.len(), before + 4, "refreshed, not stacked");
        let skin = &app.editor().doc().recipe.masks[before + 2];
        assert_eq!(skin.texture, -30.0, "its sliders stay");
        assert_eq!(skin.mask, autoshade::recipe::MaskGeometry::Bitmap { path: "mask-face-b.png".into() });
        assert_eq!(app.workspace().status(), "Found 1 face");
        // No face: the line says so and the person still lands.
        let mut none = portrait_app(Lang::En, false);
        { let (e, w, _l) = none.editor_workspace_mut(); e.on_portrait_found(w, Lang::En, PortraitFind { faces: Ok(None), ..find("c", 0) }) };
        assert_eq!(none.workspace().status(), "No face found in this photo");
        assert_eq!(none.editor().doc().recipe.masks.iter().filter(|m| m.role != autoshade::recipe::MaskRole::Custom).count(), 1, "the background alone");
        assert_eq!(none.editor().portrait_face_gate(&none.workspace(), Lang::En), Some("No face found in this photo"));
        // Both halves failed: nothing lands, the line says why.
        let mut failed = portrait_app(Lang::En, false);
        {
            let (e, w, _l) = failed.editor_workspace_mut();
            e.on_portrait_found(w, Lang::En, PortraitFind { photo: photo.clone(), subject: Err("no torch".into()), faces: Err("no mediapipe".into()) });
        }
        assert!(matches!(&failed.workspace().portrait().line, PortraitLine::Failed(t) if t.contains("no mediapipe")));
        assert_eq!(failed.editor().doc().recipe.masks.len(), before);
        // Another photo is open now: the landing is dropped.
        let mut moved = portrait_app(Lang::En, false);
        moved.editor_mut().photo_mut().src_path = Some(std::env::temp_dir().join(format!("autoshade-library-{}/_another.ARW", std::process::id())));
        { let (e, w, _l) = moved.editor_workspace_mut(); e.on_portrait_found(w, Lang::En, find("d", 1)) };
        assert_eq!(moved.editor().doc().recipe.masks.len(), before);
    }

    /// Design §4 in the GUI: with "Keep skin natural" on, a write past the
    /// limits — the user's own and an AI landing's alike — is pulled back
    /// at the frame's one place (`guard_portrait_skin`); off, nothing is
    /// clamped.
    ///
    /// MUTATION THIS CATCHES: drop the `guard_portrait_skin` call from the
    /// frame, or ignore the switch.
    #[test]
    fn keep_skin_natural_holds_user_and_ai_writes_and_off_holds_none() {
        // ON: a card inside the limits opens with the switch on, and a later
        // write past them is pulled back by a real frame of the app.
        let mut app = portrait_app(Lang::En, true);
        assert!(app.editor().keep_skin_on(&app.workspace()), "a card inside the limits opens with the switch on");
        push_skin(&mut app);
        settle(&mut app);
        let skin = autoshade::face::skin_target(&mut app.editor_mut().doc_mut().recipe).unwrap().clone();
        assert_eq!((skin.saturation, skin.sharpness, skin.texture), (15.0, 0.0, -60.0), "pulled back to the limits");
        // OFF (the user's flip): the same write stands.
        let mut free = portrait_app(Lang::En, true);
        assert!(free.editor().keep_skin_on(&free.workspace()));
        { let (e, w, l) = free.editor_workspace_mut(); e.set_keep_skin(w, l, false) };
        push_skin(&mut free);
        { let (e, w, _l) = free.editor_workspace_mut(); e.guard_portrait_skin(w) };
        let skin = autoshade::face::skin_target(&mut free.editor_mut().doc_mut().recipe).unwrap();
        assert_eq!((skin.saturation, skin.sharpness, skin.texture), (60.0, 40.0, -90.0), "off clamps nothing");
    }

    /// Three skin values past the limits on the skin mask.
    fn push_skin(app: &mut AutoShadeApp) {
        let skin = autoshade::face::skin_target(&mut app.editor_mut().doc_mut().recipe).unwrap();
        skin.saturation = 60.0;
        skin.sharpness = 40.0;
        skin.texture = -90.0;
    }

    /// Opening never changes a value: a saved version that already goes past
    /// the skin limits is read through the real `load_version` path and
    /// stays as saved, with the switch OFF for that card only; turning it on
    /// is the explicit act that brings the values back, and the status line
    /// says how many. A version inside the limits reads with the switch on,
    /// and a forked card keeps its parent's switch.
    ///
    /// MUTATION THIS CATCHES: drop `decide_keep_skin_for_card` from
    /// `load_version` (the first frame clamps the saved edit), make the
    /// switch one global value (the other card follows), drop the count from
    /// `set_keep_skin`, or drop `inherit_keep_skin`.
    #[test]
    fn a_saved_edit_beyond_the_limits_opens_unchanged_with_the_switch_off() {
        let dir = std::env::temp_dir().join(format!("autoshade-gui-keep-skin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("_gui_keep_skin.jpg");
        std::fs::write(&src, b"jpg").unwrap();
        let dev = autoshade::store::develop_dir(&src);
        let _ = std::fs::remove_dir_all(&dev);
        std::fs::create_dir_all(&dev).unwrap();
        let _scrub = Scrub(vec![dir, dev]);
        let mut past = portrait_app(Lang::En, true);
        push_skin(&mut past);
        let inside = EditRecipe { masks: found_masks(), ..Default::default() };
        for (n, r) in [(1, &past.editor().doc().recipe), (2, &inside)] {
            std::fs::write(autoshade::store::version_target(&src, n), serde_json::to_string(r).unwrap()).unwrap();
        }
        let mut app = portrait_app(Lang::En, true);
        app.editor_mut().photo_mut().src_path = Some(src.clone());
        app.commands().load_version(1);
        for _ in 0..2 {
            settle(&mut app);
        }
        let read = |app: &mut AutoShadeApp| {
            let s = autoshade::face::skin_target(&mut app.editor_mut().doc_mut().recipe).unwrap();
            (s.saturation, s.sharpness, s.texture)
        };
        assert_eq!(read(&mut app), (60.0, 40.0, -90.0), "opening changed nothing");
        assert!(!app.editor().keep_skin_on(&app.workspace()), "the switch opens off for this card");
        // Per card: the other card has no say from this one.
        let card = app.editor().doc().active;
        app.editor_mut().doc_mut().active = 0;
        assert!(app.editor().keep_skin_on(&app.workspace()), "another card is not touched");
        app.editor_mut().doc_mut().active = card;
        // Turning it on is the explicit act: three values come back, said so.
        app.editor_mut().session_mut().dirty = false;
        { let (e, w, l) = app.editor_workspace_mut(); e.set_keep_skin(w, l, true) };
        assert_eq!(read(&mut app), (15.0, 0.0, -60.0));
        assert!(app.editor().session().dirty, "an edit like any other");
        assert_eq!(app.workspace().status(), "Skin kept natural: 3 values brought back within the limits");
        assert!(app.editor().keep_skin_on(&app.workspace()));
        // A fork keeps the switch of the card it came from.
        { let (e, w, l) = app.editor_workspace_mut(); e.set_keep_skin(w, l, false) };
        let from = app.editor().doc().variants[app.editor().doc().active].id.clone();
        { let (e, w, _l) = app.editor_workspace_mut(); e.inherit_keep_skin(w, &from, "forked-card") };
        assert_eq!(app.workspace().portrait().keep_skin.get(&(src.clone(), "forked-card".to_string())), Some(&false));
        // A version inside the limits reads with the switch on, and the
        // guardrail holds a later write past them.
        app.commands().load_version(2);
        assert!(app.editor().keep_skin_on(&app.workspace()), "inside the limits: on");
        push_skin(&mut app);
        settle(&mut app);
        assert_eq!(read(&mut app), (15.0, 0.0, -60.0));
        // The photo's own open decides every card of its strip the same way.
        app.workspace_mut().portrait_mut().keep_skin.clear();
        { let (e, w, l) = app.editor_workspace_mut(); e.set_keep_skin(w, l, false) };
        push_skin(&mut app);
        { let (e, w, _l) = app.editor_workspace_mut(); e.decide_keep_skin_for_photo(w) };
        assert!(!app.editor().keep_skin_on(&app.workspace()), "the open card past the limits: off");
        app.editor_mut().doc_mut().active = 0;
        assert!(app.editor().keep_skin_on(&app.workspace()), "a card inside them: on");
    }

    /// A stand-in AI portrait look: a face mask pushed past every skin limit,
    /// the way an over-eager model might answer.
    fn portrait_look_runner(
        _photo: &std::path::Path,
        _req: &autoshade::pipeline::GradeRequest,
    ) -> anyhow::Result<(EditRecipe, autoshade::advisor::Verdict, Vec<autoshade::rationale::Note>)> {
        let face = autoshade::recipe::LocalAdjustment {
            saturation: 45.0,
            hue: 80.0,
            temperature: -60.0,
            clarity: -90.0,
            ..autoshade::face::portrait_adjustment(autoshade::recipe::MaskRole::Face, "mask-face-ai.png".into())
        };
        let recipe = EditRecipe { exposure_ev: 0.2, masks: vec![face], ..Default::default() };
        let verdict =
            autoshade::advisor::Verdict { decision: autoshade::advisor::Decision::Accept, reasons: Vec::new(), revised_hint: None };
        Ok((recipe, verdict, Vec::new()))
    }

    /// ✨ AI portrait look: the portrait direction goes out (the preset's text,
    /// the look strength, the Normal presets' request), the landing keeps the
    /// photo's found masks that the look has no mask for — with their sliders
    /// at rest — and its lens blur, and the guardrail holds the look's face
    /// mask at the same place it holds the user's sliders. All of it before
    /// the landing WRITES the look: recipe.json carries the masks, the blur
    /// and the held skin, and the saved mark is the look on screen.
    ///
    /// MUTATION THIS CATCHES: put the masks back after `on_analyzed` (the old
    /// the claimed look's `landed`) or hold the skin only on the next frame, and
    /// the file on disk is mask-less and past the limits.
    #[test]
    fn the_ai_portrait_look_asks_for_the_portrait_direction_and_lands_under_the_guardrail() {
        let (mut app, src, _scrub) = normal_photo("portrait-look");
        app.workspace_mut().normal_mut().runner = Some(portrait_look_runner);
        let mut skin = autoshade::face::skin_adjustment("mask-face.png".into(), [0.8, 0.6, 0.5], [0.4, 0.3]);
        skin.texture = -25.0;
        app.editor_mut().doc_mut().recipe.masks.push(skin);
        app.editor_mut().doc_mut().recipe.lens_blur = Some(autoshade::render::LensBlur { depth_map: "depth.png".into(), amount: 70.0, ..Default::default() });
        app.aip_mut().grade_strength = 0.4;
        app.commands().start_portrait_look();
        assert!(app.workspace().busy() && app.workspace().tools().analyze_inflight);
        let (req, guidance, library) = app.workspace().normal().last_request.clone().expect("the analysis was asked");
        assert_eq!(guidance.as_deref(), Some(autoshade::advisor::presets::PORTRAIT.direction));
        assert_eq!(req.guide, autoshade::pipeline::Guide::Direction);
        assert_eq!(req.strength.get(), 0.4, "the look strength");
        assert_eq!(library, autoshade::library::DEFAULT_NAME);
        let ctx = harness::Ctx::new();
        land_analysis(&mut app, &ctx);
        assert!(!app.workspace().busy());
        assert_eq!(app.editor().doc().recipe.exposure_ev, 0.2, "the look landed");
        let roles: Vec<autoshade::recipe::MaskRole> = app.editor().doc().recipe.masks.iter().map(|m| m.role).collect();
        assert_eq!(roles, [autoshade::recipe::MaskRole::Face, autoshade::recipe::MaskRole::Skin], "the look's face mask, and the photo's skin mask back");
        let back = &app.editor().doc().recipe.masks[1];
        assert_eq!((back.texture, back.mask.clone()), (0.0, autoshade::recipe::MaskGeometry::Bitmap { path: "mask-face.png".into() }));
        assert!(back.range.is_some(), "its colour range comes back with it");
        assert_eq!(app.editor().doc().recipe.lens_blur.as_ref().map(|l| l.amount), Some(70.0), "the blur stays");
        assert_eq!(app.workspace().portrait().line, crate::panels::portrait::PortraitLine::Looked);
        // What was WRITTEN is what the panel promises — the masks back, the
        // blur kept, the skin held — on disk, not only on the next frame's
        // screen, and the saved mark is the look on screen.
        let hue = autoshade::face::skin_hue_max();
        let disk: EditRecipe =
            serde_json::from_str(&std::fs::read_to_string(autoshade::store::recipe_target(&src)).unwrap()).unwrap();
        assert_eq!(disk.masks.iter().map(|m| m.role).collect::<Vec<_>>(), roles, "the masks came back before the write");
        assert_eq!(disk.lens_blur.as_ref().map(|l| l.amount), Some(70.0), "the blur was written too");
        let f = &disk.masks[0];
        assert_eq!((f.saturation, f.hue, f.temperature, f.clarity), (15.0, hue, -20.0, -40.0), "held before the write");
        assert_eq!(app.editor().session().hist.saved_recipe, app.editor().doc().recipe, "the saved look is the one on screen");
        { let (e, w, _l) = app.editor_workspace_mut(); e.guard_portrait_skin(w) };
        let face = &app.editor().doc().recipe.masks[0];
        assert_eq!(
            (face.saturation, face.hue, face.temperature, face.clarity),
            (15.0, hue, -20.0, -40.0),
            "the AI's face mask meets the user's limits"
        );
    }

    /// A stand-in for the billed detect + heal: three spots healed, the
    /// output a small real picture (the landing decodes it).
    fn blemish_runner(
        _src: &std::path::Path,
        limit: Option<&std::path::Path>,
        out: &std::path::Path,
    ) -> anyhow::Result<autoshade::retouch::HealReport> {
        assert!(limit.is_some(), "the panel's heal is always held to the face");
        image::RgbImage::from_pixel(6, 4, image::Rgb([90, 80, 70])).save(out)?;
        Ok(autoshade::retouch::HealReport { spots: 3, skipped: 0, rationale: String::new(), dims: (6, 4), notes: Vec::new() })
    }

    /// The photo in Portraits on an ▣ card with pixels, history reset, the
    /// heal stood in for; the panel's 🤖 Remove blemishes button found on a
    /// drawn frame.
    fn blemish_app(tag: &str, lang: Lang, face: bool) -> (AutoShadeApp, harness::Ctx, harness::Sz, usize, Option<harness::RectT>, Scrub) {
        use crate::buttons::DRAWN;
        let (mut app, _src, scrub) = normal_photo(tag);
        *app.lang_mut() = lang;
        let b0 = std::sync::Arc::new(image::DynamicImage::new_rgb8(6, 4));
        app.editor_mut().doc_mut().variants = vec![crate::model::Variant {
            id: crate::model::new_variant_id(),
            name: None,
            kind: crate::model::VariantKind::Original,
            recipe: EditRecipe::default(),
            base: Some(b0.clone()),
            origin: None,
            thumb: None,
        }];
        app.editor_mut().doc_mut().active = 0;
        app.editor_mut().photo_mut().base_preview = Some(b0);
        if face {
            app.editor_mut().doc_mut().recipe.masks.extend(found_masks());
        } else {
            app.editor_mut().doc_mut().recipe.masks.push(autoshade::face::background_adjustment("mask-subject.png".into()));
        }
        { let (e, w, _l) = app.editor_workspace_mut(); e.reset_history(w); }
        app.workspace_mut().portrait_mut().heal_runner = Some(blemish_runner);
        let mut ctx = harness::Ctx::new();
        crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
        let size = harness::sz(1400.0, 1600.0);
        let mut frame = 0;
        ctx.frame(&mut app, size, frame, Vec::new());
        frame += 1;
        DRAWN.with_borrow_mut(Vec::clear);
        ctx.frame(&mut app, size, frame, Vec::new());
        frame += 1;
        let label = tr(lang, K_PORTRAIT_REMOVE_BLEMISHES);
        let rect = DRAWN.with_borrow(|d| d.iter().find(|b| b.label == label).map(|b| harness::rect(b.rect)));
        (app, ctx, size, frame, rect, scrub)
    }

    /// No face mask (the person alone was found): 🤖 Remove blemishes is
    /// drawn greyed — a click starts nothing.
    ///
    /// MUTATION THIS CATCHES: the button's `face_raster(..).is_some()` gate
    /// dropped (a click then starts a heal with no limit to hold it to).
    #[test]
    fn remove_blemishes_is_greyed_without_a_face() {
        for lang in [Lang::En, Lang::Zh] {
            let (mut app, mut ctx, size, mut frame, rect, _scrub) = blemish_app(&format!("blemish-none-{lang:?}"), lang, false);
            assert_eq!(autoshade::face::face_raster(&app.editor().doc().recipe), None);
            let rect = rect.expect("the button is drawn, greyed");
            ctx.click(&mut app, size, &mut frame, rect.center());
            assert_eq!(app.workspace().portrait().heal_asked, None, "{lang:?}: a greyed button starts nothing");
            assert!(!app.workspace().busy());
        }
    }

    /// With a face: the click runs the Retouch panel's AI heal (no painted
    /// area) held to the face mask's raster; the landing says how many spots
    /// were healed on the face in the session language, bakes the pixels in
    /// place as one undo step (Ctrl+Z takes it back, as the Retouch heal), and
    /// "nothing on the face" lands as an answer, not a failure.
    ///
    /// MUTATION THIS CATCHES: the face raster not passed as the limit (or the
    /// painted-area path taken), the landing worded as the plain heal, or the
    /// typed no-spot answer shown as "retouch failed".
    #[test]
    fn remove_blemishes_heals_held_to_the_face_and_the_landing_counts_the_spots() {
        for lang in [Lang::En, Lang::Zh] {
            let (mut app, mut ctx, size, mut frame, rect, _scrub) = blemish_app(&format!("blemish-face-{lang:?}"), lang, true);
            let before = app.editor().doc().variants[0].base.clone().unwrap();
            ctx.click(&mut app, size, &mut frame, rect.expect("the button is drawn").center());
            assert_eq!(app.workspace().portrait().heal_asked, Some((false, Some(PathBuf::from("mask-face.png")))), "{lang:?}: the AI heal, held to the face raster");
            assert_eq!(app.workspace().status(), tr(lang, K_ACTIONS_REMOVING_BLEMISHES_ON_THE_FACE_10_30S));
            let start = std::time::Instant::now();
            while app.workspace().busy() {
                assert!(start.elapsed() < std::time::Duration::from_secs(20), "the heal never landed");
                std::thread::sleep(std::time::Duration::from_millis(20));
                app.poll_workers(&ctx);
            }
            let saved = app.editor().doc().variants[0].origin.clone().expect("the heal baked into the card");
            let _out = Scrub(vec![saved.clone()]);
            let want = trf(
                lang,
                K_LOGIC_HEALED_SPOT_S_ON_THE_FACE_SPOTS_ELSEWHERE_WERE_L,
                &[("n", "3"), ("path", &saved.display().to_string())],
            );
            assert_eq!(app.workspace().status(), want, "{lang:?}");
            assert!(app.editor().session().hist.committed.base.as_ref().is_some_and(|b| !std::sync::Arc::ptr_eq(b, &before)), "the healed pixels are the head");
            let undone = app.editor().session().hist.undo_stack.last().expect("one undo step");
            assert!(undone.base.as_ref().is_some_and(|b| std::sync::Arc::ptr_eq(b, &before)), "Ctrl+Z goes back to the pixels before");
            // Spots found, none on the face.
            app.workspace_mut().set_busy(true);
            let epoch = app.workspace().tools().gen_epoch;
            app.commands().on_retouched(&ctx, lang, epoch, Err(autoshade::retouch::NoSpotInsideLimit { found: 2 }.into()));
            assert!(!app.workspace().busy());
            assert_eq!(
                app.workspace().status(),
                trf(lang, K_LANDING_NO_SPOTS_ON_THE_FACE_TO_HEAL_NOTHING_WAS_CHANGED, &[("n", "2")])
            );
        }
    }

    /// A second search that finds the person and no face: the face, eyes and
    /// skin masks of the first search go — they would keep acting with their
    /// sliders on a face that is not found — so the groups say 「No face
    /// found」 and 🤖 Remove blemishes has no face to hold to.
    ///
    /// MUTATION THIS CATCHES: leave the old face-role masks in place on a
    /// no-face landing (`on_portrait_found`), and the masks, the gate and the
    /// heal's face raster all stay.
    #[test]
    fn a_search_that_finds_no_face_takes_the_old_face_masks_away() {
        use crate::logic::portrait::{FoundFaces, PortraitFind};
        let mut app = portrait_app(Lang::En, true);
        let photo = app.editor().photo().src_path.clone().unwrap();
        autoshade::face::set_soften(autoshade::face::skin_target(&mut app.editor_mut().doc_mut().recipe).unwrap(), 30.0);
        assert!(autoshade::face::face_raster(&app.editor().doc().recipe).is_some());
        {
            let (e, w, _l) = app.editor_workspace_mut();
            e.on_portrait_found(w, Lang::En, PortraitFind {
                photo: photo.clone(),
                subject: Ok(PathBuf::from("mask-subject-2.png")),
                faces: Ok(None::<FoundFaces>),
            });
        }
        let roles: Vec<autoshade::recipe::MaskRole> = app.editor().doc().recipe.masks.iter().map(|m| m.role).collect();
        for gone in [autoshade::recipe::MaskRole::Face, autoshade::recipe::MaskRole::Eyes, autoshade::recipe::MaskRole::Skin] {
            assert!(!roles.contains(&gone), "{gone:?} is gone: {roles:?}");
        }
        assert!(roles.contains(&autoshade::recipe::MaskRole::Background), "the person still lands");
        assert_eq!(app.editor().portrait_face_gate(&app.workspace(), Lang::En), Some("No face found in this photo"));
        assert_eq!(autoshade::face::face_raster(&app.editor().doc().recipe), None, "🤖 Remove blemishes has no face");
    }

    /// Ctrl+Z back to an edit made past the limits (before the switch was
    /// turned on) puts it back as it was: the switch goes off for the card,
    /// the next frame does not pull it back, and Ctrl+Y still has its step.
    ///
    /// MUTATION THIS CATCHES: leave the switch on after an undo restores a
    /// value past the limits — the frame clamps it, `commit_now` records a new
    /// step and the redo stack is cleared.
    #[test]
    fn undo_to_an_edit_past_the_skin_limits_puts_it_back_and_keeps_redo() {
        let mut app = portrait_app(Lang::En, true);
        app.editor_mut().doc_mut().active = 0;
        let ctx = harness::Ctx::new();
        { let (e, w, _l) = app.editor_workspace_mut(); e.reset_history(&*w) };
        { let (e, w, l) = app.editor_workspace_mut(); e.set_keep_skin(w, l, false) };
        push_skin(&mut app);
        { let (e, w, _l) = app.editor_workspace_mut(); e.commit_now(&*w) };
        { let (e, w, l) = app.editor_workspace_mut(); e.set_keep_skin(w, l, true) };
        { let (e, w, _l) = app.editor_workspace_mut(); e.commit_now(&*w) };
        let read = |app: &mut AutoShadeApp| {
            let s = autoshade::face::skin_target(&mut app.editor_mut().doc_mut().recipe).unwrap();
            (s.saturation, s.sharpness, s.texture)
        };
        assert_eq!(read(&mut app), (15.0, 0.0, -60.0), "turned on: held");
        { let (e, w, l) = app.editor_workspace_mut(); e.undo(w, l, &ctx) };
        settle(&mut app);
        assert_eq!(read(&mut app), (60.0, 40.0, -90.0), "undone as it was");
        assert!(!app.editor().keep_skin_on(&app.workspace()), "the switch follows the edit put back");
        assert_eq!(app.editor().session().hist.redo_stack.len(), 1, "the step forward is still there");
    }

    /// Ctrl+Y after that Ctrl+Z gives back what the user had before it: the
    /// values within the limits AND the switch on, so the next frame records
    /// nothing and Ctrl+Z still walks back.
    ///
    /// MUTATION THIS CATCHES: drop the switch from the history step (or stop
    /// `apply_step` putting it back) and the redo leaves the switch off.
    #[test]
    fn redo_after_switching_the_skin_guard_on_turns_it_back_on() {
        let mut app = portrait_app(Lang::En, true);
        app.editor_mut().doc_mut().active = 0;
        let ctx = harness::Ctx::new();
        { let (e, w, _l) = app.editor_workspace_mut(); e.reset_history(&*w) };
        { let (e, w, l) = app.editor_workspace_mut(); e.set_keep_skin(w, l, false) };
        push_skin(&mut app);
        { let (e, w, _l) = app.editor_workspace_mut(); e.commit_now(&*w) };
        { let (e, w, l) = app.editor_workspace_mut(); e.set_keep_skin(w, l, true) };
        { let (e, w, _l) = app.editor_workspace_mut(); e.commit_now(&*w) };
        let read = |app: &mut AutoShadeApp| {
            let s = autoshade::face::skin_target(&mut app.editor_mut().doc_mut().recipe).unwrap();
            (s.saturation, s.sharpness, s.texture)
        };
        { let (e, w, l) = app.editor_workspace_mut(); e.undo(w, l, &ctx) };
        settle(&mut app);
        assert!(!app.editor().keep_skin_on(&app.workspace()), "undone: off");
        { let (e, w, l) = app.editor_workspace_mut(); e.redo(w, l, &ctx) };
        settle(&mut app);
        assert_eq!(read(&mut app), (15.0, 0.0, -60.0), "redone: held");
        assert!(app.editor().keep_skin_on(&app.workspace()), "redone: the switch is on again");
        assert_eq!((app.editor().session().hist.undo_stack.len(), app.editor().session().hist.redo_stack.len()), (2, 0), "the frame recorded nothing");
        { let (e, w, l) = app.editor_workspace_mut(); e.undo(w, l, &ctx) };
        assert_eq!(read(&mut app), (60.0, 40.0, -90.0), "Ctrl+Z still walks back");
        assert!(!app.editor().keep_skin_on(&app.workspace()));
    }

    /// A flip of the switch that changes no value is not a step of its own:
    /// it belongs to the step on screen, so undo and redo of a later value
    /// edit leave it as the user set it, and a value edit made while held
    /// comes back held.
    ///
    /// MUTATION THIS CATCHES: let `commit_now` keep the switch the head was
    /// recorded with when a flip records no step — the Ctrl+Z below turns
    /// the switch the user turned off back on.
    #[test]
    fn undo_and_redo_of_a_value_edit_keep_the_switch_the_user_set() {
        let mut app = portrait_app(Lang::En, true);
        app.editor_mut().doc_mut().active = 0;
        let ctx = harness::Ctx::new();
        { let (e, w, _l) = app.editor_workspace_mut(); e.reset_history(&*w) };
        assert!(app.editor().keep_skin_on(&app.workspace()));
        let edit = |app: &mut AutoShadeApp, v: f32| {
            autoshade::face::skin_target(&mut app.editor_mut().doc_mut().recipe).unwrap().saturation = v;
        };
        edit(&mut app, 5.0);
        { let (e, w, _l) = app.editor_workspace_mut(); e.commit_now(&*w) };
        { let (e, w, l) = app.editor_workspace_mut(); e.undo(w, l, &ctx) };
        { let (e, w, l) = app.editor_workspace_mut(); e.redo(w, l, &ctx) };
        assert!(app.editor().keep_skin_on(&app.workspace()), "a value edit made while held comes back held");
        { let (e, w, l) = app.editor_workspace_mut(); e.set_keep_skin(w, l, false) };
        { let (e, w, _l) = app.editor_workspace_mut(); e.commit_now(&*w) };
        assert_eq!(app.editor().session().hist.undo_stack.len(), 1, "the flip records no step");
        edit(&mut app, 10.0);
        { let (e, w, _l) = app.editor_workspace_mut(); e.commit_now(&*w) };
        { let (e, w, l) = app.editor_workspace_mut(); e.undo(w, l, &ctx) };
        assert!(!app.editor().keep_skin_on(&app.workspace()), "undo of a later edit keeps the switch off");
        { let (e, w, l) = app.editor_workspace_mut(); e.redo(w, l, &ctx) };
        assert!(!app.editor().keep_skin_on(&app.workspace()), "and so does redo");
    }

    /// After Ctrl+Z past the switch-on, a new edit starts a new branch: the
    /// redo step (the switch-on) is gone, and the switch stays off.
    ///
    /// MUTATION THIS CATCHES: keep the redo stack across a new step in
    /// `commit_now`.
    #[test]
    fn a_new_edit_after_undoing_the_switch_on_drops_its_redo() {
        let mut app = portrait_app(Lang::En, true);
        app.editor_mut().doc_mut().active = 0;
        let ctx = harness::Ctx::new();
        { let (e, w, _l) = app.editor_workspace_mut(); e.reset_history(&*w) };
        { let (e, w, l) = app.editor_workspace_mut(); e.set_keep_skin(w, l, false) };
        push_skin(&mut app);
        { let (e, w, _l) = app.editor_workspace_mut(); e.commit_now(&*w) };
        { let (e, w, l) = app.editor_workspace_mut(); e.set_keep_skin(w, l, true) };
        { let (e, w, _l) = app.editor_workspace_mut(); e.commit_now(&*w) };
        { let (e, w, l) = app.editor_workspace_mut(); e.undo(w, l, &ctx) };
        autoshade::face::skin_target(&mut app.editor_mut().doc_mut().recipe).unwrap().saturation = 70.0;
        { let (e, w, _l) = app.editor_workspace_mut(); e.commit_now(&*w) };
        assert!(app.editor().session().hist.redo_stack.is_empty(), "the new edit drops the redo branch");
        assert!(!app.editor().keep_skin_on(&app.workspace()), "the switch stays off");
        { let (e, w, l) = app.editor_workspace_mut(); e.undo(w, l, &ctx) };
        assert!(!app.editor().keep_skin_on(&app.workspace()), "undoing the new edit puts back an edit past the limits: off");
    }

    /// A paste in Portraits meets each target's switch where it is written:
    /// a target whose switch is on gets the look within the limits on disk;
    /// the open photo, its switch off, gets it as copied.
    ///
    /// MUTATION THIS CATCHES: write the pasted look without the per-target
    /// guard (`start_paste_to`) and the other photo's recipe.json goes past
    /// the limits.
    #[test]
    fn a_paste_in_portraits_holds_each_targets_skin_on_disk() {
        let dir = std::env::temp_dir().join(format!("autoshade-gui-paste-skin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (open, other) = (dir.join("_gui_paste_skin_a.jpg"), dir.join("_gui_paste_skin_b.jpg"));
        let devs: Vec<PathBuf> = [&open, &other].iter().map(|p| autoshade::store::develop_dir(p)).collect();
        for d in &devs {
            let _ = std::fs::remove_dir_all(d);
        }
        let mut scrub = devs.clone();
        scrub.push(dir);
        let _scrub = Scrub(scrub);
        let mut app = portrait_app(Lang::En, false);
        app.editor_mut().doc_mut().active = 0;
        app.editor_mut().photo_mut().src_path = Some(open.clone());
        let skin = autoshade::recipe::LocalAdjustment {
            role: autoshade::recipe::MaskRole::Skin,
            saturation: 60.0,
            sharpness: 40.0,
            texture: -90.0,
            ..Default::default()
        };
        let look = EditRecipe { masks: vec![skin], ..Default::default() };
        { let (e, w, l) = app.editor_workspace_mut(); e.set_keep_skin(w, l, false) };
        app.commands().start_paste_to(look, Some(open.clone()), vec![open.clone(), other.clone()], false);
        let ctx = harness::Ctx::new();
        let start = std::time::Instant::now();
        while app.workspace().busy() {
            assert!(start.elapsed() < std::time::Duration::from_secs(20), "the paste never landed");
            std::thread::sleep(std::time::Duration::from_millis(20));
            app.poll_workers(&ctx);
        }
        let disk = |p: &std::path::Path| -> (f32, f32, f32) {
            let r: EditRecipe =
                serde_json::from_str(&std::fs::read_to_string(autoshade::store::recipe_target(p)).unwrap()).unwrap();
            let m = r.masks.iter().find(|m| m.role == autoshade::recipe::MaskRole::Skin).expect("the skin mask pasted");
            (m.saturation, m.sharpness, m.texture)
        };
        assert_eq!(disk(&other), (15.0, 0.0, -60.0), "the other photo's switch is on: held on disk");
        assert_eq!(disk(&open), (60.0, 40.0, -90.0), "the open photo's switch is off: as copied");
    }

    /// The find step seeds the skin colour from the pixels the skin mask's
    /// colour range is matched on — the photo developed under the recipe up
    /// to the skin mask (`face::skin_seed_reference`) — not the neutral frame.
    ///
    /// MUTATION THIS CATCHES: seed from `base.to_rgb8()` again in
    /// `panel_skin_seed` and both assertions below fail.
    #[test]
    fn the_skin_seed_is_sampled_from_the_frame_the_recipe_develops_before_the_skin_mask() {
        let base = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(32, 24, image::Rgb([180, 140, 120])));
        let face = image::GrayImage::from_pixel(32, 24, image::Luma([255]));
        let mut recipe = EditRecipe { temperature_k: Some(3200.0), tint: 80.0, ..Default::default() };
        recipe.masks.push(found_masks()[2].clone());
        let seed = crate::portrait_actions::panel_skin_seed(&base, &face, &recipe, None, false, None).expect("a seed");
        let reference = autoshade::face::skin_seed_reference(&base, &recipe, 0, None, false, None);
        assert_eq!(Some(seed), autoshade::face::skin_seed(&reference, &face), "seeded from the developed reference");
        let neutral = autoshade::face::skin_seed(&base.to_rgb8(), &face).unwrap();
        let moved = (0..3).map(|c| (seed.0[c] - neutral.0[c]).abs()).fold(0.0f32, f32::max);
        assert!(moved > 0.02, "the white balance moved the seed off the neutral frame's ({moved})");
    }
