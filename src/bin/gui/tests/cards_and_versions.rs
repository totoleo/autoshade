// One part of the GUI's tests (src/bin/gui/tests.rs includes it): variant cards, edit states, versions and the paste, batch and export plumbing they ride on.

    #[test]
    fn geometry_tiles_paste_while_photo_keyed_components_stay_home() {
        use autoshade::recipe::{LocalAdjustment, MaskCombine, MaskComponent, MaskGeometry};
        let tile = LocalAdjustment {
            name: "Spatial tile r1c2".into(),
            components: (0..3).map(|_| MaskComponent { mode: MaskCombine::Intersect, ..Default::default() }).collect(),
            ..Default::default()
        };
        let raster_component = LocalAdjustment {
            components: vec![MaskComponent {
                geometry: MaskGeometry::Bitmap { path: "refined.png".into() }, ..Default::default()
            }], ..Default::default()
        };
        let zone_component = LocalAdjustment {
            role: autoshade::recipe::MaskRole::ZoneSky,
            components: vec![MaskComponent {
                geometry: MaskGeometry::select_sky(0.5, 0.25, false, "source-sky.png".into()),
                mode: MaskCombine::Intersect, inverted: false,
            }], ..Default::default()
        };
        let recipe = EditRecipe { masks: vec![tile.clone(), raster_component, zone_component], ..Default::default() };
        let paste = crate::export::paste_payload(recipe.clone(), true);
        assert_eq!(paste.unpastable_masks, 2);
        assert_eq!(paste.foreign.masks, vec![tile]);
        assert_eq!(paste.own.masks, recipe.masks);
        assert!(autoshade::xmp::mask_export_losses(&paste.foreign).is_empty());
        assert!(!xmp_loss_interrupts(&autoshade::xmp::mask_export_losses(&paste.foreign), &[]));
    }

    /// R25 P8: what a paste is allowed to CARRY.
    ///
    /// The pass-through map is per-DOCUMENT state — Lightroom's Transform /
    /// Upright block and the camera profile name, read off ONE photo's sidecar
    /// and authored by nothing in this app. Carried to another photo it is
    /// provenance pollution with teeth: the target's next save would write the
    /// SOURCE's Upright solution into the file beside a RAW it was never
    /// solved for, and the read-only Transform / camera-profile section would
    /// show the wrong photo's values the moment the paste landed.
    ///
    /// The clipboard's OWN photo keeps it, on the bitmap-mask rule beside it:
    /// there the map really is that photo's own state.
    #[test]
    fn a_paste_carries_the_edit_and_not_the_source_documents_own_state() {
        use crate::export::paste_payload;
        let mut src = EditRecipe {
            exposure_ev: 0.75,
            straighten_deg: 3.0,
            crop: Some(autoshade::recipe::Crop {
                top: 0.1,
                left: 0.1,
                bottom: 0.9,
                right: 0.9,
            }),
            ..Default::default()
        };
        src.passthrough = [("PerspectiveVertical", "-35"), ("CameraProfile", "Adobe Standard")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        src.masks.push(autoshade::recipe::LocalAdjustment {
            mask: autoshade::recipe::MaskGeometry::Bitmap { path: "sky.png".into() },
            ..Default::default()
        });
        // A reverse-fit ZONE is not a `Bitmap` any more — it rides Lightroom's
        // own Select Sky component — but its alpha is still a file in THIS
        // photo's develop dir, so it travels no better than one.
        src.masks.push(autoshade::recipe::LocalAdjustment {
            mask: autoshade::recipe::MaskGeometry::select_sky(
                0.5,
                0.25,
                false,
                "mask-zone-sky.png".into(),
            ),
            role: autoshade::recipe::MaskRole::ZoneSky,
            ..Default::default()
        });
        // …while an IMPORTED Lightroom AI mask carries only intent and
        // re-segments against whatever photo it lands on, so it does travel.
        src.masks.push(autoshade::recipe::LocalAdjustment {
            mask: autoshade::recipe::MaskGeometry::select_sky(0.5, 0.25, false, "cached.png".into()),
            ..Default::default()
        });
        // R33 §G: per-photo in the strongest sense a global control can be.
        // Its twelve-by-eight cells are normalised to the frame it was solved
        // on, so cell (3, 1) means "this scene's left-of-centre sky" and
        // nothing at all on another photograph.
        src.colour_field = Some(autoshade::recipe::ColourField {
            x: 2,
            y: 2,
            b: 2,
            grid: vec![[0.1, 0.0, 0.0, 0.0, 0.0]; 8],
            amount: 1.0,
            enabled: true,
        });

        let p = paste_payload(src.clone(), true);
        assert!(
            p.foreign.passthrough.is_empty(),
            "another photo's Upright block must not ride along: {:?}",
            p.foreign.passthrough
        );
        assert_eq!(p.own.passthrough, src.passthrough, "the source photo keeps its own");
        assert_eq!(p.foreign.exposure_ev, 0.75, "the EDIT is what a paste is for");
        // The rule this one was modelled on, still holding — and it counts by
        // "whose pixels are these", not by geometry variant: the Bitmap and the
        // ZONE both stay behind, the imported AI mask rides along.
        assert_eq!(p.unpastable_masks, 2, "the photo-keyed rasters are counted for the toast");
        assert_eq!(p.foreign.masks.len(), 1, "…and dropped from the foreign payload");
        assert!(
            matches!(
                &p.foreign.masks[0].mask,
                autoshade::recipe::MaskGeometry::AiMask { raster: Some(r), .. } if r == "cached.png"
            ) && p.foreign.masks[0].role == autoshade::recipe::MaskRole::Custom,
            "the survivor is the re-derivable AI mask: {:?}",
            p.foreign.masks[0].mask
        );
        assert_eq!(p.own.masks.len(), 3, "…while the clipboard's own photo keeps them all");
        // The colour field is the same kind of loss and is reported the same
        // way: counted, never silently dropped.
        assert!(p.colour_field, "the dropped field is reported for the toast");
        assert!(p.foreign.colour_field.is_none(), "…and is gone from the foreign payload");
        assert!(p.own.colour_field.is_some(), "…while its own photo keeps it");
        // Geometry off strips BOTH arms: composition rarely transfers between
        // frames, and that includes back onto the source.
        let g = paste_payload(src, false);
        assert_eq!((g.own.crop.is_some(), g.own.straighten_deg), (false, 0.0));
        assert_eq!((g.foreign.crop.is_some(), g.foreign.straighten_deg), (false, 0.0));
    }

    /// L16-2: the batch resolver surfaces the disclosures the OPEN path
    /// would — a silent-neutral XMP value must reach the batch outcome.
    #[test]
    fn the_batch_resolver_discloses_what_the_open_path_would() {
        let dir = std::env::temp_dir()
            .join(format!("autoshade-batch-warns-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("photo.arw");
        std::fs::write(&src, b"raw").unwrap();
        let xmp = r#"<rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="+1.00" crs:Contrast2012="bogus"/>"#;
        let snap = autoshade::store::DevelopSnapshot {
            recipe: None,
            recipe_err: None,
            lr_xmp: Some((xmp.to_string(), "Lightroom sidecar")),
            store_xmp: None,
            lr_unreadable: None,
            packet_xmp: None,
            packet_unreadable: None,
            pixel_source: None,
            pixel_recorded: false,
        };
        let mut warns = Vec::new();
        let (r, kind) = crate::export::resolve_snapshot_develop(&src, &snap, &mut warns)
            .unwrap()
            .expect("a non-noop sidecar resolves");
        assert_eq!(kind, "Lightroom sidecar");
        assert!((r.exposure_ev - 1.0).abs() < 1e-3);
        assert!(
            warns.iter().any(|w| w.contains("unreadable") && w.contains("Contrast2012")),
            "the silent neutral is disclosed on the batch surface: {warns:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// L16-9: every export dial reaches the renderer's options — this block
    /// could previously be deleted with every GUI test green.
    #[test]
    fn export_opts_carries_every_dial_to_the_renderer() {
        let app = AutoShadeApp::default().but_workspace({ let mut view = WorkspaceView::default(); *view.exp_mut() = ExportState {
                long_edge: 2560,
                sharpen: 35.0,
                quality: 88.0,
                space: 1,
                format: ExportFormat::Png8,
                ..Default::default()
            }; view });
        let o = app.workspace().export_opts();
        assert_eq!(o.long_edge, Some(2560));
        assert!((o.sharpen - 35.0).abs() < 1e-6);
        assert_eq!(o.jpeg_quality, 88);
        assert!(o.eight_bit, "Png8 is 8-bit");
        assert!(matches!(o.color_space, autoshade::render::ExportColorSpace::DisplayP3));
        let flat = AutoShadeApp::default().but_workspace({ let mut view = WorkspaceView::default(); *view.exp_mut() = ExportState {
                long_edge: 0,
                ..Default::default()
            }; view });
        assert_eq!(flat.workspace().export_opts().long_edge, None, "0 = no resize");
    }

    /// L16-11: the shortcut exclusivity gates, tested NEGATIVELY — deleting
    /// the transient guard used to fail nothing. Covers the Settings window and
    /// the reverse-fit question; Esc dismisses a transient instead of leaking.
    #[test]
    fn a_transient_window_swallows_canvas_shortcuts() {
        let ctx = egui::Context::default();
        let key = |k| egui::Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        // Positive control first: with NO transient up, R arms the crop —
        // proving this harness actually reaches the tool tier.
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {

                src_path: Some(std::env::temp_dir().join(format!("autoshade-library-{}/x.arw", std::process::id()))),

                ..Default::default()

            }; editor });
        let mut input = egui::RawInput::default();
        input.events.push(key(egui::Key::R));
        let _ = ctx.run(input, |ctx| app.upd_shortcuts(ctx));
        assert!(app.workspace().crop().mode, "positive control: R arms the crop tool");

        // The negative: a transient (Settings) swallows the tool tier…
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {

                src_path: Some(std::env::temp_dir().join(format!("autoshade-library-{}/x.arw", std::process::id()))),

                ..Default::default()

            }; editor }).but_workspace({ let mut view = WorkspaceView::default(); *view.ui_mut() = ShellState { show_settings: true, ..Default::default() }; view });
        let mut input = egui::RawInput::default();
        input.events.push(key(egui::Key::R));
        let _ = ctx.run(input, |ctx| app.upd_shortcuts(ctx));
        assert!(!app.workspace().crop().mode, "R is dead while a modal transient is up");
        // …and Esc dismisses the transient instead of leaking anywhere.
        let mut input = egui::RawInput::default();
        input.events.push(key(egui::Key::Escape));
        let _ = ctx.run(input, |ctx| app.upd_shortcuts(ctx));
        assert!(!app.workspace().ui().show_settings, "Esc dismissed the transient");
        assert!(!app.workspace().crop().mode);

        // The reverse-fit question is a transient too: R is dead under it
        // (the prompt's own frame path is the same gate).
        let one = "out/_fit_prompt_test.reimagine.png";
        let mut prompt = app_before_a_generation(Lang::En);
        let land_ctx = harness::Ctx::new();
        land(&mut prompt, &land_ctx, reimagined(one), one, RetouchKind::NewGenerated);
        assert!(prompt.workspace().fit().fit_prompt.is_some());
        let mut input = egui::RawInput::default();
        input.events.push(key(egui::Key::R));
        let _ = ctx.run(input, |ctx| prompt.upd_shortcuts(ctx));
        assert!(!prompt.workspace().crop().mode, "R is dead under the prompt");
    }

    /// L16-12: the display resync is exercised through a REAL swap path
    /// (load_version), not by calling the helper directly.
    #[test]
    fn a_version_load_resyncs_the_display_state() {
        let dir = std::env::temp_dir()
            .join(format!("autoshade-version-resync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("photo.arw");
        std::fs::write(&src, b"raw").unwrap();
        let dev = autoshade::store::develop_dir(&src);
        let _ = std::fs::remove_dir_all(&dev);
        std::fs::create_dir_all(&dev).unwrap();
        let versioned = EditRecipe {
            exposure_ev: 0.75,
            rationale: "v1 marker rationale".into(),
            ..Default::default()
        };
        std::fs::write(
            autoshade::store::version_target(&src, 1),
            serde_json::to_string(&versioned).unwrap(),
        )
        .unwrap();

        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {

                src_path: Some(src.clone()),

                ..Default::default()

            }; *editor.analysis_mut() = AnalysisSession {


                // stale index into the OLD mask list
rationale: "stale rationale".into(),


                ..Default::default()


            }; editor }).but_workspace({ let mut view = WorkspaceView::default(); *view.masks_mut() = MaskEditState {
                sel_mask: Some(3),
                ..Default::default()
            }; view });
        app.commands().load_version(1);
        assert!((app.editor().doc().recipe.exposure_ev - 0.75).abs() < 1e-3, "the version landed");
        assert_eq!(
            app.editor().analysis().rationale, "v1 marker rationale",
            "the swap path resynced the rationale display"
        );
        assert_eq!(app.workspace().masks().sel_mask, None, "a stale mask selection cannot linger");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dev);
    }

    /// R24-2: a variant's identity + name survive every hop between the live
    /// strip, the navigation stash and `variants.json`. Six hops carry them,
    /// and a name that falls off ANY of them is silent loss — the user typed
    /// it, nothing warned, and the card comes back nameless.
    #[test]
    fn a_variant_name_survives_every_hop_it_has_to_take() {
        use crate::model::{StashedVariant, Variant, VariantKind};
        let mk = |kind, id: &str, name: Option<&str>| Variant {
            kind,
            id: id.into(),
            name: name.map(str::to_string),
            recipe: EditRecipe::default(),
            base: None,
            origin: None,
            thumb: None,
        };
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.doc_mut() = Document {

                variants: vec![
                mk(VariantKind::Original, "card-a", Some("base")),
                mk(VariantKind::Fitted, "card-b", Some("偏暖")),
            ],

                active: 1,

                ..Default::default()

            }; editor });

        // Hop 1 (live strip → record) and hop 2 (record → live strip).
        let rec = app.editor().current_strip_record().expect("a two-card strip is worth recording");
        assert_eq!(rec.active_id.as_deref(), Some("card-b"));
        assert_eq!(rec.active_name.as_deref(), Some("偏暖"));
        assert_eq!(rec.others[0].id.as_deref(), Some("card-a"));
        assert_eq!(rec.others[0].name.as_deref(), Some("base"));
        let (back, _) = crate::persist::strip_from_record(&rec, None);
        assert_eq!(back[0].id, "card-a");
        assert_eq!(back[0].name.as_deref(), Some("base"));

        // A LEGACY record (no ids — every strip written before R24-2) is
        // minted one on the way in, or the versions taken from that card
        // could never be attributed again.
        let legacy = autoshade::store::VariantsRecord {
            extra: Default::default(),
            v: 1,
            active_kind: "original".into(),
            active_pos: 0,
            active_id: None,
            active_name: None,
            others: vec![autoshade::store::VariantEntry {
                extra: Default::default(),
                kind: "generated".into(),
                recipe: EditRecipe::default(),
                origin: None,
                id: None,
                name: None,
            }],
        };
        let (minted, _) = crate::persist::strip_from_record(&legacy, None);
        assert!(!minted[0].id.is_empty(), "an id-less legacy entry is minted an identity");

        // Hop 3 (live strip → stash) and hop 5 (stash → record), driven
        // through the stash-building path the way navigation does.
        let others: Vec<StashedVariant> = app.editor().doc().variants
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != app.editor().doc().active)
            .map(|(_, v)| StashedVariant {
                kind: v.kind,
                id: v.id.clone(),
                name: v.name.clone(),
                recipe: v.recipe.clone(),
                base: v.base.clone(),
                origin: v.origin.clone(),
            })
            .collect();
        let st = crate::model::StashEntry {
            recipe: app.editor().doc().recipe.clone(),
            base: None,
            origin: None,
            kind: app.editor().doc().variants[app.editor().doc().active].kind,
            id: app.editor().doc().variants[app.editor().doc().active].id.clone(),
            name: app.editor().doc().variants[app.editor().doc().active].name.clone(),
            others,
            active_pos: app.editor().doc().active,
        };
        let stashed = crate::util::stash_strip_record(&st).expect("a stashed strip is recordable");
        assert_eq!(stashed.active_id.as_deref(), Some("card-b"));
        assert_eq!(stashed.active_name.as_deref(), Some("偏暖"));
        assert_eq!(stashed.others[0].id.as_deref(), Some("card-a"));
        assert_eq!(stashed.others[0].name.as_deref(), Some("base"));

        // Hop 4 (stash → live strip): the shape the Opened handler rebuilds.
        let restored: Vec<Variant> = st
            .others
            .into_iter()
            .map(|sv| Variant {
                kind: sv.kind,
                id: sv.id,
                name: sv.name,
                recipe: sv.recipe,
                base: sv.base,
                origin: sv.origin,
                thumb: None,
            })
            .collect();
        assert_eq!(restored[0].id, "card-a");
        assert_eq!(restored[0].name.as_deref(), Some("base"));

        // TRIVIALITY, R24-3: the cards became renameable, so a lone Original
        // is no longer trivial by its card count alone — the record is the
        // ONLY home its name and its minted id have, and dropping it would
        // discard both silently (the `strip_is_trivial` owner, shared with
        // the stash path).
        app.editor_mut().doc_mut().variants.truncate(1);
        app.editor_mut().doc_mut().active = 0;
        let rec = app
            .editor().current_strip_record()
            .expect("a NAMED lone Original still needs its record");
        assert_eq!(rec.active_name.as_deref(), Some("base"));
        assert_eq!(rec.active_id.as_deref(), Some("card-a"));
        // Strip the two things only the record can hold and it goes back to
        // being noise: recipe.json + pixels.json say everything, and the base
        // negative's fixed id is reconstructed by every reader.
        app.editor_mut().doc_mut().variants[0].name = None;
        app.editor_mut().doc_mut().variants[0].id = crate::model::ORIGINAL_VARIANT_ID.to_string();
        assert!(app.editor().current_strip_record().is_none());
    }

    /// R24-3 (#7): 「apply to Original」 copies a card's develop PARAMETERS
    /// onto the base negative — and nothing else. Its baked pixels, its
    /// raster origin and the SOURCE card all survive (Lightroom's 「Set Copy
    /// as Original」 rule), and the whole thing is exactly one Ctrl+Z.
    #[test]
    fn applying_a_variant_to_the_original_copies_parameters_and_nothing_else() {
        use crate::model::{Variant, VariantKind};
        let ctx = egui::Context::default();
        let pixels = Arc::new(image::DynamicImage::new_rgb8(8, 6));
        let master = PathBuf::from("out/_apply_master.png");
        let negative = Variant {
            kind: VariantKind::Original,
            id: crate::model::ORIGINAL_VARIANT_ID.into(),
            name: None,
            recipe: EditRecipe { contrast: 11.0, ..Default::default() },
            base: Some(pixels.clone()),
            origin: Some(master.clone()),
            thumb: None,
        };
        let fitted = Variant {
            kind: VariantKind::Fitted,
            id: "card-fit".into(),
            name: Some("dusk".into()),
            recipe: EditRecipe { contrast: 44.0, exposure_ev: 0.5, ..Default::default() },
            base: None,
            origin: None,
            thumb: None,
        };
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.doc_mut() = Document {

                variants: vec![negative, fitted],

                active: 1,

                recipe: EditRecipe { contrast: 44.0, exposure_ev: 0.5, ..Default::default() },

                ..Default::default()

            }; editor });
        { let (e, w, _l) = app.editor_workspace_mut(); e.reset_history(&*w) };
        app.commands().apply_to_original(1, &ctx);

        assert_eq!(app.editor().doc().active, 0, "the canvas lands on the card that was written");
        assert_eq!(app.editor().doc().variants.len(), 2, "the source card is KEPT");
        assert_eq!(app.editor().doc().variants[1].name.as_deref(), Some("dusk"), "…untouched");
        assert_eq!(app.editor().doc().variants[0].recipe.contrast, 44.0, "the develop was copied");
        assert_eq!(app.editor().doc().variants[0].recipe.exposure_ev, 0.5);
        assert!(
            app.editor().doc().variants[0].base.as_ref().is_some_and(|b| Arc::ptr_eq(b, &pixels)),
            "the negative's own pixels are not touched"
        );
        assert_eq!(
            app.editor().doc().variants[0].origin.as_deref(),
            Some(master.as_path()),
            "…nor its raster origin: this copies PARAMETERS"
        );
        assert_eq!(app.editor().doc().recipe.contrast, 44.0, "the canvas shows what was applied");

        // ONE Ctrl+Z, and the negative's own develop is back.
        { let (e, w, l) = app.editor_workspace_mut(); e.undo(w, l, &ctx) };
        assert_eq!(app.editor().doc().recipe.contrast, 11.0, "one undo step, not zero and not two");

        // A PIXEL-STATE source is refused with the reverse-fit remedy — its
        // look is in its raster, and the recipe over it is stripped bare.
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.doc_mut() = Document {

                variants: vec![
                Variant {
                    kind: VariantKind::Original,
                    id: crate::model::ORIGINAL_VARIANT_ID.into(),
                    name: None,
                    recipe: EditRecipe { contrast: 11.0, ..Default::default() },
                    base: None,
                    origin: None,
                    thumb: None,
                },
                Variant {
                    kind: VariantKind::Generated,
                    id: "card-gen".into(),
                    name: None,
                    recipe: EditRecipe::default(),
                    base: None,
                    origin: Some(PathBuf::from("out/_gen.png")),
                    thumb: None,
                },
            ],

                active: 1,

                ..Default::default()

            }; editor });
        app.commands().apply_to_original(1, &ctx);
        assert_eq!(app.editor().doc().variants[0].recipe.contrast, 11.0, "the negative was not overwritten");
        assert_eq!(app.editor().doc().active, 1, "and the canvas did not move");
        assert!(
            app.workspace().status().contains("Reverse-fit"),
            "the refusal carries the remedy: {}",
            app.workspace().status()
        );
    }

    /// R24-4 (#1, phase 2 — minimal): ONE list answers "what edit states does
    /// this photo have" — the rendition CARDS first, the numbered SNAPSHOTS
    /// under them. The order is the claim (a photo is one negative + N
    /// variants + one version history), so the test pins it.
    #[test]
    fn the_edit_state_list_puts_the_cards_above_the_versions() {
        use crate::model::{Variant, VariantKind};
        let mk = |kind, id: &str| Variant {
            kind,
            id: id.into(),
            name: None,
            recipe: EditRecipe::default(),
            base: None,
            origin: None,
            thumb: None,
        };
        let ctx = egui::Context::default();
        crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.doc_mut() = Document {

                variants: vec![
                mk(VariantKind::Original, crate::model::ORIGINAL_VARIANT_ID),
                mk(VariantKind::Generated, "card-gen"),
            ],

                active: 0,

                ..Default::default()

            }; editor }).but_workspace({ let mut view = WorkspaceView::default(); *view.cards_mut() = CardsState {
                versions: vec![1, 2],
                ..Default::default()
            }; view });
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| {
                // Versions is a collapsed section by default; egui's own test
                // hook opens every collapsible (the brush-slider test's idiom).
                ctx.memory_mut(|m| m.set_everything_is_visible(true));
                egui::SidePanel::left("controls").default_width(320.0).show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| app.commands().develop_panel(ui));
                });
            },
        );
        assert_eq!(
            app.seams().edit_list_rows,
            vec!["variant:original", "variant:generated", "version:1", "version:2"],
            "cards above versions, in strip order then ascending"
        );
    }

    /// R24-5: the edit-state list's rows CARRY the card actions — the same
    /// ＋ / ▣ / ✕ the strip draws, through the one owner
    /// (`variant_card_buttons`), so 「what does ✕ do here」 cannot become two
    /// answers. What this pins:
    ///
    ///   * per-row membership: ＋ and ▣ on the ACTIVE card only (both act on
    ///     the live canvas), ✕ on any droppable card, none on a lone Original;
    ///   * ▣ DISABLED rather than hidden on a pixel-state card — the R24-2
    ///     judgement, and the reason it is a seam at all;
    ///   * the panel does not grow: this section gained two widgets per row in
    ///     a 320 px panel, and an unbounded row is how the side panel crept
    ///     +8 px a frame twice before (R19, R22-3).
    ///
    /// The STRIP's own seams are untouched by a frame that draws both, which
    /// is why `strip_apply` stays the strip's (`VariantSurface`).
    #[test]
    fn the_edit_state_rows_carry_the_card_actions_without_widening_the_panel() {
        use crate::model::{Variant, VariantKind};
        let mk = |kind, id: &str| Variant {
            kind,
            id: id.into(),
            name: None,
            recipe: EditRecipe::default(),
            base: None,
            origin: None,
            thumb: None,
        };
        for (active, want) in [
            // The Generated card is active: it offers ＋, a DISABLED ▣ (its
            // look lives in its pixels), and ✕. The background Original
            // offers nothing — it is neither the live canvas nor droppable.
            (1usize, vec!["1:＋▣(off)✕"]),
            // The Original is active: ＋ only. ▣ is hidden (it IS the
            // negative) and the Original is never droppable; the background
            // Generated keeps its ✕.
            (0usize, vec!["0:＋", "1:✕"]),
        ] {
            let ctx = egui::Context::default();
            crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
            let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.doc_mut() = Document {

                    variants: vec![
                    mk(VariantKind::Original, crate::model::ORIGINAL_VARIANT_ID),
                    mk(VariantKind::Generated, "card-gen"),
                ],

                    active,

                    ..Default::default()

                }; editor }).but_workspace({ let mut view = WorkspaceView::default(); *view.cards_mut() = CardsState {
                    versions: vec![1],
                    ..Default::default()
                }; view });
            let mut widths = Vec::new();
            for _ in 0..3 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1400.0, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        ctx.memory_mut(|m| m.set_everything_is_visible(true));
                        let r = egui::SidePanel::left("controls")
                            .default_width(320.0)
                            .show(ctx, |ui| {
                                egui::ScrollArea::vertical().show(ui, |ui| app.commands().develop_panel(ui));
                            });
                        widths.push(r.response.rect.width());
                    },
                );
            }
            assert_eq!(app.seams().edit_list_actions, want, "active={active}");
            assert!(
                (widths[0] - widths[2]).abs() < 0.5,
                "active={active}: the controls panel must not grow ({widths:?})"
            );
        }

        // A lone Original: nothing is droppable and nothing else exists, so
        // the only action is ＋. (A ✕ here would offer to delete the photo's
        // one edit state.)
        let ctx = egui::Context::default();
        crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.doc_mut() = Document {

                variants: vec![mk(VariantKind::Original, crate::model::ORIGINAL_VARIANT_ID)],

                active: 0,

                ..Default::default()

            }; editor });
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| {
                ctx.memory_mut(|m| m.set_everything_is_visible(true));
                egui::SidePanel::left("controls").default_width(320.0).show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| app.commands().develop_panel(ui));
                });
            },
        );
        assert_eq!(app.seams().edit_list_actions, vec!["0:＋"]);
    }

    /// R24-3/R24-4, on the real panel: the strip's ACTIVE card carries the
    /// two new affordances — a name box and 「apply to Original」 — and a
    /// PIXEL-STATE card shows the apply button DISABLED (with the reverse-fit
    /// reason) rather than hiding it. Headless `Context::run`, never a window.
    #[test]
    fn the_active_card_offers_its_name_box_and_the_apply_button() {
        use crate::model::{Variant, VariantKind};
        let mk = |kind, id: &str| Variant {
            kind,
            id: id.into(),
            name: None,
            recipe: EditRecipe::default(),
            base: None,
            origin: None,
            thumb: None,
        };
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            ..Default::default()
        };
        let photo = std::env::temp_dir().join(format!("autoshade-library-{}/_strip_affordances.ARW", std::process::id()));
        for (kind, expect_enabled) in
            [(VariantKind::Fitted, true), (VariantKind::Generated, false)]
        {
            let ctx = egui::Context::default();
            crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
            let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {

                    src_path: Some(photo.clone()),

                    ..Default::default()

                }; *editor.doc_mut() = Document {


                    variants: vec![
                    mk(VariantKind::Original, crate::model::ORIGINAL_VARIANT_ID),
                    mk(kind, "card-b"),
                ],


                    active: 1,


                    ..Default::default()


                }; editor });
            let _ = ctx.run(input(), |ctx| {
                egui::TopBottomPanel::bottom("variants")
                    .exact_height(AutoShadeApp::VARIANT_STRIP_H)
                    .show(ctx, |ui| app.commands().variant_strip(ui));
            });
            let (rect, enabled) = app
                .seams().strip_apply
                .unwrap_or_else(|| panic!("{kind:?}: the active card has no apply button"));
            assert!(rect.width() > 0.0, "{kind:?}: the apply button did not lay out");
            assert_eq!(
                enabled, expect_enabled,
                "{kind:?}: a pixel-state card must SHOW the button disabled, not hide it"
            );
            // The name affordance is there too, and a seeded buffer turns it
            // into the edit box (the version rows' shape, on the card's id).
            assert!(
                app.seams().strip_name_rect.is_some_and(|r| r.width() > 0.0),
                "{kind:?}: the active card offers no name affordance"
            );
            app.workspace_mut().cards_mut().variant_name_buf =
                Some((photo.clone(), "card-b".into(), String::new(), "dusk".into()));
            app.seams().strip_name_rect = None;
            let _ = ctx.run(input(), |ctx| {
                egui::TopBottomPanel::bottom("variants")
                    .exact_height(AutoShadeApp::VARIANT_STRIP_H)
                    .show(ctx, |ui| app.commands().variant_strip(ui));
            });
            assert!(
                app.seams().strip_name_rect.is_some_and(|r| r.width() > 0.0),
                "{kind:?}: the seeded rename box did not lay out"
            );
        }
    }

    /// R24-3: a card rename lands on the CARD it was typed on — the buffer is
    /// keyed by the card's own id, so an async push that renumbers the strip
    /// mid-typing cannot paint the name onto a different card. And it counts
    /// as unsaved work: the strip record is the name's only home.
    #[test]
    fn a_card_rename_follows_its_card_and_counts_as_unsaved() {
        use crate::model::{Variant, VariantKind};
        let mk = |kind, id: &str| Variant {
            kind,
            id: id.into(),
            name: None,
            recipe: EditRecipe::default(),
            base: None,
            origin: None,
            thumb: None,
        };
        let photo = std::env::temp_dir().join(format!("autoshade-library-{}/_rename_card.ARW", std::process::id()));
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {

                src_path: Some(photo.clone()),

                ..Default::default()

            }; *editor.doc_mut() = Document {


                variants: vec![
                mk(VariantKind::Original, crate::model::ORIGINAL_VARIANT_ID),
                mk(VariantKind::Fitted, "card-fit"),
            ],


                active: 1,


                ..Default::default()


            }; editor });
        // Typed on the Fitted card…
        app.workspace_mut().cards_mut().variant_name_buf =
            Some((photo.clone(), "card-fit".into(), String::new(), "dusk".into()));
        // …while an async completion inserts a card ahead of it.
        app.editor_mut().doc_mut().variants.insert(1, mk(VariantKind::Generated, "card-gen"));
        app.editor_mut().doc_mut().active = 2;
        app.commands().commit_pending_names();
        assert_eq!(app.editor().doc().variants[1].name, None, "the renumbered neighbour is untouched");
        assert_eq!(
            app.editor().doc().variants[2].name.as_deref(),
            Some("dusk"),
            "the name follows the card's id, not its index"
        );

        // Unsaved-work accounting: the mirror still holds the pre-rename
        // record, so quitting has to warn.
        app.editor_mut().session_mut().hist.saved_strip = app.editor().current_strip_record();
        assert_eq!(app.editor().open_dirty_variants(), 0, "premise: the mirror is current");
        app.editor_mut().doc_mut().variants[2].name = Some("dawn".into());
        assert!(app.editor().open_dirty_variants() >= 1, "renaming the ACTIVE card is unsaved work");
        app.editor_mut().doc_mut().variants[2].name = Some("dusk".into());
        app.editor_mut().doc_mut().variants[0].name = Some("negative".into());
        assert!(app.editor().open_dirty_variants() >= 1, "…and so is renaming a background card");
    }

    /// R24-3: the calibration rule has TWO directions and one owner. Onto a
    /// pixel-state card a snapshot's calibration is stripped; a snapshot
    /// TAKEN off one arrives with none at all, and landing it on the negative
    /// used to leave the photo rendering with no camera base look — the
    /// pre-era repair declines it (current era stamp, curve under three
    /// knots), so nothing else could have healed it.
    #[test]
    fn a_snapshot_off_a_generated_card_gets_the_negatives_calibration_back() {
        let knots = vec![[0.0, 0.0], [0.5, 0.6], [1.0, 1.0]];
        let lens = autoshade::recipe::LensProfile {
            distortion: vec![0.02, 0.0, 0.0],
            distortion_on: true,
            ..Default::default()
        };
        let cal = || (autoshade::pipeline::CanvasStart { base_curve: knots.clone() }, lens.clone(), Some((5200.0, 3.0)));

        // The defect's direction: a snapshot with no calibration, onto the
        // parametric negative.
        let mut r = EditRecipe { contrast: 7.0, ..Default::default() };
        assert!(
            crate::persist::reconcile_snapshot_calibration(&mut r, false, cal),
            "the stamp is reported so the load can disclose it"
        );
        assert_eq!(r.base_curve.len(), 3, "the photo's own camera base look is back");
        assert_eq!(r.lens_profile.distortion, vec![0.02, 0.0, 0.0], "…the lens profile too");
        assert_eq!(r.as_shot_k, Some(5200.0), "…and the as-shot anchor with it");
        assert_eq!(r.contrast, 7.0, "the user's edits are not touched");

        // A snapshot that BROUGHT its own calibration is left exactly alone —
        // legacy develops must render as they were tuned.
        let mut own = EditRecipe {
            base_curve: vec![[0.0, 0.0], [0.5, 0.4], [1.0, 1.0]],
            ..Default::default()
        };
        let before = own.clone();
        assert!(!crate::persist::reconcile_snapshot_calibration(&mut own, false, cal));
        assert_eq!(own, before, "a snapshot with its own curve is untouched");

        // …and the other direction still strips, anchor included.
        let mut onto_pixels = EditRecipe {
            base_curve: vec![[0.0, 0.0], [1.0, 1.0]],
            lens_profile: lens.clone(),
            as_shot_k: Some(5200.0),
            as_shot_tint: Some(3.0),
            ..Default::default()
        };
        assert!(!crate::persist::reconcile_snapshot_calibration(&mut onto_pixels, true, cal));
        assert!(onto_pixels.base_curve.is_empty(), "baked pixels carry the look already");
        assert_eq!(onto_pixels.lens_profile, autoshade::recipe::LensProfile::default());
        assert_eq!(onto_pixels.as_shot_k, None, "…and a baked white balance");

        // R24 round-end NIT-4: the era stamp rides WITH the curve, the same
        // rule `pipeline::stamp_fit_calibration` and the open path follow. A
        // snapshot is the only input that arrives carrying an OLDER era, and
        // an era-1 stamp over knots THIS build just estimated made the
        // caller's `repair_pre_era_base_curve` "re-estimate" a curve that was
        // already fresh — and say so on screen.
        let mut legacy = EditRecipe { version: 1, contrast: 7.0, ..Default::default() };
        assert!(crate::persist::reconcile_snapshot_calibration(&mut legacy, false, cal));
        assert_eq!(
            legacy.version,
            autoshade::recipe::CALIB_ERA,
            "a freshly estimated curve carries this build's era"
        );
        assert!(
            !autoshade::pipeline::base_curve_is_pre_era(legacy.version, &legacy.base_curve),
            "…so the pre-era repair declines it instead of announcing a re-estimate"
        );
        // A calibration that produced NO knots stamps no curve, so it makes no
        // era claim either.
        let mut none = EditRecipe { version: 1, ..Default::default() };
        assert!(!crate::persist::reconcile_snapshot_calibration(&mut none, false, || (
            autoshade::pipeline::CanvasStart::default(),
            autoshade::recipe::LensProfile::default(),
            None
        )));
        assert_eq!(none.version, 1, "no curve stamped, no era restated");
    }

    /// R24-2: 「＋ Save as version」 on a PIXEL-STATE card wrote a near-empty
    /// recipe — the canvas over a generated raster carries no curve, no lens
    /// profile and no as-shot anchor by construction, so the snapshot
    /// restored to nothing. Same judgement as Ctrl+S's XMP refusal.
    #[test]
    fn saving_a_version_off_a_generated_variant_is_refused_not_emptied() {
        let dir = std::env::temp_dir()
            .join(format!("autoshade-genversion-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("photo.arw");
        std::fs::write(&src, b"raw").unwrap();
        let dev = autoshade::store::develop_dir(&src);
        let _ = std::fs::remove_dir_all(&dev);
        std::fs::create_dir_all(&dev).unwrap();

        let mk = |kind| crate::model::Variant {
            kind,
            id: crate::model::new_variant_id(),
            name: None,
            recipe: EditRecipe::default(),
            base: None,
            origin: Some(dir.join("master.png")),
            thumb: None,
        };
        let mut app = AutoShadeApp::default().but_editor({ let mut editor = Editor::default(); *editor.photo_mut() = PhotoPixels {

                src_path: Some(src.clone()),

                ..Default::default()

            }; *editor.doc_mut() = Document {


                variants: vec![mk(crate::model::VariantKind::Generated)],


                ..Default::default()


            }; editor });
        app.commands().save_version();
        assert!(
            autoshade::store::list_versions(&src).is_empty(),
            "a generated card must not mint an empty snapshot"
        );
        assert!(
            app.workspace().chrome().toasts.iter().any(|t| matches!(t.kind, ToastKind::Error)),
            "the refusal must be SEEN — the keyboard path has no other channel"
        );

        // The same canvas on a PARAMETRIC card saves, and records what it
        // was a picture of.
        app.workspace_mut().chrome_mut().toasts.clear();
        app.editor_mut().doc_mut().variants = vec![crate::model::Variant {
            id: "card-fit".into(),
            ..mk(crate::model::VariantKind::Fitted)
        }];
        app.commands().save_version();
        assert_eq!(autoshade::store::list_versions(&src), vec![1]);
        let meta = autoshade::store::read_version_meta(&src);
        assert_eq!(meta[0].from_kind.as_deref(), Some("fitted"));
        assert_eq!(meta[0].from_id.as_deref(), Some("card-fit"));
        assert_eq!(meta[0].origin.as_deref(), Some(autoshade::store::VERSION_ORIGIN_USER));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dev);
    }
