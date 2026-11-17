// One part of the GUI's tests (src/bin/gui/tests.rs includes it): the AI lens
// blur section (2026-10-13) — the depth landing, the focus pick's arming,
// and the two places a per-photo depth map must not travel.

    /// A first depth estimate lands a blur on the card and arms the focus
    /// pick (disarming any other canvas tool); a re-estimate swaps the map
    /// and keeps the focus and dials the photographer chose; a failure says
    /// so and leaves the card as it was.
    #[test]
    fn a_depth_estimate_lands_a_blur_and_arms_the_focus_pick() {
        let mut app = AutoShadeApp::default().but_workspace({ let mut view = WorkspaceView::default(); *view.tools_mut() = ToolsState { wb_picking: true, ..Default::default() }; *view.busy_mut() = true; view });
        let depth = std::env::temp_dir().join(format!("autoshade-lens-{}/depth.png", std::process::id()));
        app.commands().on_depth_estimated(Ok(depth.clone()));
        let lb = app.editor().doc().recipe.lens_blur.clone().expect("the first estimate lands a blur");
        assert_eq!(lb.depth_map, depth.display().to_string());
        assert!(app.workspace().tools().lens_focus_picking, "a first estimate arms the focus pick");
        assert!(!app.workspace().tools().wb_picking, "one canvas tool at a time");
        assert!(!app.workspace().busy() && app.editor().session().dirty);

        let chosen = autoshade::render::LensBlur { focus: [0.2, 0.8], amount: 70.0, range: 30.0, ..lb };
        app.editor_mut().doc_mut().recipe.lens_blur = Some(chosen.clone());
        app.workspace_mut().tools_mut().lens_focus_picking = false;
        let depth2 = std::env::temp_dir().join(format!("autoshade-lens-{}/depth-2.png", std::process::id()));
        app.commands().on_depth_estimated(Ok(depth2.clone()));
        let again = app.editor().doc().recipe.lens_blur.clone().unwrap();
        assert_eq!(again.depth_map, depth2.display().to_string());
        assert_eq!((again.focus, again.amount, again.range), (chosen.focus, chosen.amount, chosen.range));
        assert!(!app.workspace().tools().lens_focus_picking, "a re-estimate does not re-arm the pick");

        let before = app.editor().doc().recipe.clone();
        app.commands().on_depth_estimated(Err(anyhow::anyhow!("no GPU")));
        assert_eq!(app.editor().doc().recipe, before, "a failed estimate changes nothing");
    }

    /// A depth map measures one photo's scene: pasting onto another photo
    /// drops the blur (and says so), and a turn is refused while any card
    /// carries one, as for a fill layer.
    #[test]
    fn a_lens_blur_neither_pastes_across_nor_survives_a_turn() {
        let blur = autoshade::render::LensBlur { depth_map: "depth.png".into(), ..Default::default() };
        let src = EditRecipe { exposure_ev: 0.5, lens_blur: Some(blur.clone()), ..Default::default() };
        let p = crate::export::paste_payload(src, true);
        assert!(p.lens_blur && p.foreign.lens_blur.is_none(), "another photo gets the edit without the blur");
        assert_eq!(p.own.lens_blur.as_ref(), Some(&blur), "the source photo keeps it");

        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {

                src_path: Some(std::env::temp_dir().join(format!("autoshade-library-{}/turn.ARW", std::process::id()))),

                base_preview: Some(std::sync::Arc::new(image::DynamicImage::new_rgb8(6, 4))),

                ..Default::default()

            }; editor });
        assert!(app.editor().can_rotate(&app.workspace()), "premise: the photo is rotatable without a blur");
        app.editor_mut().doc_mut().recipe.lens_blur = Some(blur);
        assert!(!app.editor().can_rotate(&app.workspace()), "a depth map estimated in this frame must block the turn");
    }
