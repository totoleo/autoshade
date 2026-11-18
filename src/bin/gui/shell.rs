//! The frame shell: the toolbar, status bar, side panels, overlays and sheets
//! one frame draws. The root owns the frame loop and the worker pump; this
//! file owns what a frame looks like, on the same command context a panel uses.
//!
//! A shell draw reaches the state through `Commands` and runs its verbs there
//! too, so the root stays wiring and the drawing stops growing the root.

use crate::*;

impl Commands<'_> {
    /// One update() phase — body extracted verbatim from the eframe
    /// update loop (round-12 decomposition).
    pub(crate) fn upd_dropped_and_title(&mut self, ctx: &Context) {
        let dropped: Vec<PathBuf> = if self.workspace.modal_up() {
            Vec::new()
        } else {
            ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect())
        };
        let n_dropped = dropped.len();
        if let Some(p) = dropped.into_iter().next() {
            if self.workspace.busy() {
                self.workspace.toast(
                    ToastKind::Error,
                    tr(self.lang, K_APP_BUSY_WAIT_FOR_THE_CURRENT_TASK_TO_FINISH_BEFORE),
                );
            } else if p.is_dir() {
                self.open_folder(p);
                if n_dropped > 1 {
                    // EVERY branch discloses the leftovers — silently eating
                    // them read as "it handled them all" (the photo branch
                    // alone used to say so).
                    self.workspace.toast(
                        ToastKind::Error,
                        trf(
                            self.lang,
                            K_APP_OPENED_THE_FIRST_FOLDER_MORE_DROPPED_ITEM_S_IGNO,
                            &[("n", &(n_dropped - 1).to_string())],
                        ),
                    );
                }
            } else if is_photo_path(&p) {
                self.workspace.gal_mut().selected = None;
                self.open_path(p);
                if n_dropped > 1 {
                    // Silently eating the rest read as "it opened them all".
                    self.workspace.toast(
                        ToastKind::Error,
                        trf(
                            self.lang,
                            K_APP_OPENED_THE_FIRST_PHOTO_MORE_IGNORED_DROP_THEIR_F,
                            &[("n", &(n_dropped - 1).to_string())],
                        ),
                    );
                }
            } else {
                let msg = if n_dropped > 1 {
                    trf(
                        self.lang,
                        K_APP_UNSUPPORTED_FILE_TYPE_MORE_DROPPED_ITEM_S_IGNORE,
                        &[
                            ("path", &p.display().to_string()),
                            ("n", &(n_dropped - 1).to_string()),
                        ],
                    )
                } else {
                    trf(self.lang, K_APP_UNSUPPORTED_FILE_TYPE, &[("path", &p.display().to_string())])
                };
                self.workspace.toast(ToastKind::Error, msg);
            }
        }

        // Window title mirrors the open photo (send only on change).
        let title = match &self.editor.photo().src_path {
            Some(p) => format!(
                "{} — AutoShade",
                p.file_name().and_then(|s| s.to_str()).unwrap_or("photo")
            ),
            None => "AutoShade".to_string(),
        };
        if title != self.workspace.chrome().last_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.workspace.chrome_mut().last_title = title;
        }
    }

    /// One update() phase — body extracted verbatim from the eframe
    /// update loop (round-12 decomposition).
    pub(crate) fn upd_top_bar(&mut self, ctx: &Context) {
        // egui's side_top_panel frame keeps only 2px of vertical margin —
        // at that spacing the action row visually touches the strip edges.
        window_chrome::edge_resize(ctx);
        window_chrome::window_outline(ctx);
        let frame = Frame::side_top_panel(&ctx.style())
            .inner_margin(Margin::symmetric(8, SPACE_MD.round() as i8))
            .fill(self.workspace.ui().theme.colors().bar_fill);
        TopBottomPanel::top("top").frame(frame).show(ctx, |ui| {
            let lang = self.lang;
            // ONE wrapped row of ACTIONS only (UX batch): settings that used
            // to live here moved to where they are used — export options into
            // the Develop panel's Export section, the AI direction/refine/
            // style trio into the AI section. The row wraps, never clips (the
            // "shrink the window and lose the buttons" bug); wrapping only
            // works between ATOMIC widget allocations, so the enabled gating
            // stays per-widget (ui.add_enabled), and groups are fenced with
            // add_space (a separator that wraps to a line start orphans).
            // AI Analyze moved INTO the AI section, onto the same row as
            // the Direction prompt it consumes (user feedback: a trigger
            // stranded in the toolbar, far from its input, reads as
            // unrelated — every prompt entry now carries its own button).
            let ready = self.editor.photo().src_path.is_some() && !self.workspace.busy();
            // R38: the row wraps only BETWEEN its groups — each group is one
            // measured allocation (`buttons::group`, sized from its labels),
            // so a narrow window moves a whole group to the next line instead
            // of splitting Undo from Redo; a plain `ui.horizontal` would be
            // laid at the cursor at whatever width was left and never wrap.
            // The group fence sits INSIDE each group's end, where a wrapped
            // line start cannot orphan it. Export is the row's one primary
            // verb; Settings and the shortcut sheet are glyph squares.
            // On Windows the row is also the title bar (`window_chrome`).
            window_chrome::toolbar_row(ui, lang, |ui| {
                group(ui, &[tr(lang, K_APP_OPEN_PHOTO), tr(lang, K_APP_RESET)], 0, |ui| {
                    if action(ui, !self.workspace.busy(), tr(lang, K_APP_OPEN_PHOTO))
                        .on_hover_text(tr(lang, K_APP_CTRL_O_OR_DRAG_A_FILE_INTO_THE_WINDOW))
                        .clicked()
                    {
                        self.workspace.gal_mut().selected = None; // a one-off file isn't a gallery selection
                        self.apply_actions(vec![logic::Action::Gallery(
                            logic::gallery::GalleryAction::OpenPhotoFile,
                        )]);
                    }
                    if action(ui, ready, tr(lang, K_APP_RESET))
                        .on_hover_text(tr(lang, K_APP_BACK_TO_THIS_PHOTO_S_FRESH_OPEN_LOOK_SLIDERS_NEU))
                        .clicked()
                    {
                        // Reset = the fresh-open look: sliders neutral on this
                        // photo's camera-matched base. RE-STAMP the open knots
                        // rather than keeping the canvas curve — a legacy save
                        // deliberately carries none, and preserving that emptiness
                        // made Reset stay dark on every previously-edited photo.
                        // Only a GENERATED variant keeps no curve (its pixels
                        // carry the look); an InPlace retouch master is a NEUTRAL
                        // develop that still needs the calibration on top —
                        // stripping there turned Reset into "go dark".
                        let generated = self.editor.active_on_ai_pixels();
                        // The fresh START (v1.17.0): the canvas profile and
                        // Look, or the camera curve where they cannot render.
                        let start =
                            if generated { Default::default() } else { self.workspace.open().photo_start.clone() };
                        // The lens profile is the same kind of calibration: Reset
                        // re-stamps the photo's own (a Generated master's pixels
                        // already carry the corrections — none re-applied).
                        let lens_profile =
                            if generated { Default::default() } else { self.workspace.open().photo_lens.clone() };
                        // The as-shot anchor follows the SAME generated strip:
                        // baked pixels carry their WB, so an absolute-Kelvin
                        // claim over them would be false.
                        let (as_shot_k, as_shot_tint) = match self.workspace.open().photo_as_shot {
                            Some((k, t)) if !generated => (Some(k), Some(t)),
                            _ => (None, None),
                        };
                        let mut fresh = EditRecipe {
                            lens_profile,
                            as_shot_k,
                            as_shot_tint,
                            ..EditRecipe::default()
                        };
                        let start: autoshade::pipeline::CanvasStart = start;
                        start.stamp(&mut fresh);
                        self.editor.doc_mut().recipe = fresh;
                        self.workspace.overlay_mut().region = None;
                        self.editor.session_mut().dirty = true;
                        // The old AI rationale/verdict describe the recipe Reset
                        // just discarded — showing them under neutral sliders lies.
                        self.editor.resync_recipe_display(self.workspace);
                    }
                });
                group(ui, &[tr(lang, K_APP_UNDO), tr(lang, K_APP_REDO)], 0, |ui| {
                    if action(ui, ready && !self.editor.session().hist.undo_stack.is_empty(), tr(lang, K_APP_UNDO))
                        .on_hover_text(tr(lang, K_APP_CTRL_Z_UNDO_THE_LAST_EDIT))
                        .clicked()
                    {
                        let ctx = ui.ctx().clone();
                        self.editor.undo(self.workspace, self.lang, &ctx);
                    }
                    if action(ui, ready && !self.editor.session().hist.redo_stack.is_empty(), tr(lang, K_APP_REDO))
                        .on_hover_text(tr(lang, K_APP_CTRL_Y_REDO_THE_UNDONE_EDIT))
                        .clicked()
                    {
                        let ctx = ui.ctx().clone();
                        self.editor.redo(self.workspace, self.lang, &ctx);
                    }
                });
                group(ui, &[tr(lang, K_APP_TURN_LEFT), tr(lang, K_APP_TURN_RIGHT)], 0, |ui| {
                    // Rotate (R27). NOT ↶/↷ — those two glyphs are Undo/Redo one
                    // button to the left, and ↺/↻ are already 「reset all」/
                    // 「Clear」/「↻ Redraw」 in the panels. ⭯/⭮ (U+2B6F/U+2B6E) are
                    // unused anywhere in the tree and both ship in
                    // NotoSansSymbols2-autoshade.ttf, so no subset regeneration.
                    let can_rotate = self.editor.can_rotate(self.workspace);
                    // Disabled buttons state their reason (the pixel-state rule):
                    // a control that greys out in silence reads as a broken app.
                    // Spelled at each site rather than through a helper — the
                    // i18n audit reads `tr()` LEXICALLY, and a key that reaches it
                    // through a closure parameter is a key it cannot check.
                    let blocked = tr(
                        lang,
                        K_APP_ROTATION_IS_OFF_WHILE_THIS_PHOTO_CARRIES_RENDERE,
                    );
                    let left_hover = if can_rotate {
                        tr(lang, K_APP_A_QUARTER_TURN_ANTICLOCKWISE_THE_CROP_AND_EVERY)
                    } else {
                        blocked
                    };
                    if action(ui, can_rotate, tr(lang, K_APP_TURN_LEFT))
                        .on_hover_text(left_hover)
                        .clicked()
                    {
                        let ctx = ui.ctx().clone();
                        self.editor.rotate_photo(self.workspace, self.lang, 3, &ctx); // anticlockwise = three clockwise
                    }
                    let right_hover = if can_rotate {
                        tr(lang, K_APP_A_QUARTER_TURN_CLOCKWISE_THE_CROP_AND_EVERY_MASK)
                    } else {
                        blocked
                    };
                    if action(ui, can_rotate, tr(lang, K_APP_TURN_RIGHT))
                        .on_hover_text(right_hover)
                        .clicked()
                    {
                        let ctx = ui.ctx().clone();
                        self.editor.rotate_photo(self.workspace, self.lang, 1, &ctx);
                    }
                });
                group(ui, &[tr(lang, K_APP_COMPARE), tr(lang, K_APP_SINGLE)], 0, |ui| {
                    // View mode: side-by-side vs a full-width edit (hold B =
                    // compare). ◫ lives in egui's bundled fonts — the old ⿲
                    // (U+2FF2) only rendered when the OPTIONAL CJK fallback font
                    // loaded, tofu otherwise.
                    ui.selectable_value(&mut self.workspace.view_mut().mode, ViewMode::SideBySide, tr(lang, K_APP_COMPARE))
                        .on_hover_text(tr(lang, K_APP_BEFORE_AFTER_SIDE_BY_SIDE));
                    ui.selectable_value(&mut self.workspace.view_mut().mode, ViewMode::AfterOnly, tr(lang, K_APP_SINGLE))
                        .on_hover_text(tr(lang, K_APP_THE_EDIT_FILLS_THE_CANVAS_HOLD_B_TO_QUICKLY_COMP));
                });
                group(ui, &[tr(lang, K_APP_EXPORT), "▾", tr(lang, K_APP_SAVE_DEVELOP)], 0, |ui| {
                    // Delivery ACTIONS (their settings live in the Develop panel's
                    // Export section; the hover echoes the current delivery state
                    // so it stays glanceable without a toolbar row of combos).
                    //
                    // ONE split button (R22-7). 「Export」 and 「Download…」 were the
                    // same code path — start_render_to — differing only in where
                    // the target came from, which made "where do my files go" a
                    // property of WHICH BUTTON you pressed instead of a setting.
                    // The main half now follows the Destination setting; the ▾ half
                    // is the one-off escape hatch that leaves the setting alone.
                    // Zero item_spacing joins the two halves visually into one
                    // control (egui has no split-button widget).
                    let summary = self.editor.export_summary(self.workspace, lang);
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if primary(ui, ready, tr(lang, K_APP_EXPORT))
                            .on_hover_text(format!(
                                "{}\n{summary}",
                                tr(lang, K_APP_CTRL_SHIFT_E_OR_CTRL_E_FULL_RESOLUTION_RENDER_TO)
                            ))
                            .clicked()
                        {
                            self.start_export();
                        }
                        // `add_enabled_ui`, not a disabled Button: menu_button
                        // returns an InnerResponse and takes no enabled flag.
                        ui.enabled(ready, |ui| {
                            ui.menu_button("▾", |ui| {
                                if ui
                                    .button(tr(lang, K_APP_EXPORT_TO))
                                    .on_hover_text(tr(lang, K_APP_PICK_A_PATH_FOR_THIS_EXPORT_ONLY_THE_DESTINATION))
                                    .clicked()
                                {
                                    ui.close();
                                    self.export_to_chosen_path();
                                }
                            })
                            .response
                            .on_hover_text(tr(lang, K_APP_EXPORT_TO_A_ONE_OFF_PATH));
                        });
                    });
                    if action(ui, ready, tr(lang, K_APP_SAVE_DEVELOP))
                        .on_hover_text(tr(lang, K_APP_CTRL_S_SAVE_THIS_PHOTO_S_DEVELOP_RECIPE_A_LIGHTR))
                        .clicked()
                    {
                        self.save_xmp();
                    }
                });
                group(ui, &[], 2, |ui| {
                    if glyph(ui, true, "⚙").on_hover_text(tr(lang, K_APP_SETTINGS_AI_PROVIDER_MODEL_API_KEY)).clicked() {
                        // Reload the form only on the closed→open edge — reloading
                        // while open wiped everything already typed (incl. keys).
                        // Toggle semantics, matching ⌨ next door.
                        if !self.workspace.ui().show_settings {
                            self.load_settings_form();
                        }
                        self.workspace.ui_mut().show_settings = !self.workspace.ui().show_settings;
                    }
                    if glyph(ui, true, "⌨").on_hover_text(tr(lang, K_APP_KEYBOARD_SHORTCUTS_F1)).clicked() {
                        self.workspace.ui_mut().show_shortcuts = !self.workspace.ui().show_shortcuts;
                    }
                });
            });
        });
    }

    /// One update() phase — body extracted verbatim from the eframe
    /// update loop (round-12 decomposition).
    pub(crate) fn upd_status_bar(&mut self, ctx: &Context) {
        let frame = Frame::side_top_panel(&ctx.style()).fill(self.workspace.ui().theme.colors().bar_fill);
        TopBottomPanel::bottom("status").frame(frame).show(ctx, |ui| {
            ui.row(|ui| {
                if self.workspace.busy() {
                    ui.spinner();
                    // Armed by every cancellable long-run: retouch/generative
                    // workers (which poll the flag and really stop), local
                    // compute, and AI analyze — the last two are ABANDON-only,
                    // which the hover text says rather than promising a
                    // checkpoint they do not have.
                    if self.workspace.tools().gen_cancel.is_some()
                        && action(ui, true, tr(self.lang, K_APP_CANCEL))
                            .on_hover_text(tr(
                                self.lang,
                                K_APP_STOP_WAITING_THE_APP_UNBLOCKS_NOW_AND_THE_LATE_R,
                            ))
                            .clicked()
                    {
                        self.cancel_generative();
                    }
                }
                // The unsaved marker: canvas differs from the saved develop
                // (base_curve excluded — calibration is not an edit, dirty_vs)
                // OR its pixels differ from that same card's recorded master.
                // `active_baseline` resolves both halves after card switches;
                // an unsaved heal is work even under an untouched recipe. It sits FIRST
                // — before the batch bar and the status text — so neither can
                // ever push it out of view at narrow widths.
                if self.editor.unsaved_marker_dirty() {
                    ui.label(
                        Text::new(tr(self.lang, K_APP_UNSAVED))
                            .color(ui.visuals().warn_fg_color),
                    )
                    .on_hover_text(tr(
                        self.lang,
                        K_APP_THIS_CARD_DIFFERS_FROM_ITS_SAVED_DEVELOP_CTRL_S,
                    ));
                }
                // Live batch-render progress, beside the spinner (its old home
                // at the START of the toolbar shifted every control right by
                // ~160px exactly while the user was waiting).
                if let Some((done, total)) = self.workspace.exp().batch_progress {
                    ui.add(
                        ProgressBar::new(done as f32 / total.max(1) as f32)
                            .desired_width(150.0)
                            .text(trf(self.lang, K_APP_BATCH,
                                &[("done", &done.to_string()), ("total", &total.to_string())])),
                    );
                }
                // Long messages (paths, batch reports) must clip, not blow the
                // panel wide; the full text is one hover away.
                ui.add(Label::new(self.workspace.status()).truncate())
                    .on_hover_text(self.workspace.status());
            });
        });
    }

    /// One update() phase — body extracted verbatim from the eframe
    /// update loop (round-12 decomposition).
    /// The controls panel's one scroll column: the AI, Develop and Retouch
    /// panels with a photo open, the empty state without.
    ///
    /// The column keeps a gutter on its right as wide as the scrollbar and a
    /// breath more (user report 2026-10-12: the Generative Fill hint ran under
    /// the bar). egui 0.29's bar FLOATS over the content and reserves no width
    /// of its own, so every row used to fill the panel to its last pixel. The
    /// gutter is a margin, not the scroll style's `floating_allocated_width`:
    /// that one reserves only while the bar shows, and a side panel sizes
    /// itself to its content, so the panel shrank a few pixels on every frame
    /// the bar faded in.
    pub(crate) fn controls_column(&mut self, ui: &mut Ui) {
        let gutter = controls_gutter(ui.style());
        ScrollArea::vertical().show(ui, |ui| {
            Frame::NONE
                .inner_margin(Margin { right: gutter.round() as i8, ..Default::default() })
                .show(ui, |ui| {
                    if self.editor.photo().src_path.is_some() {
                        // The one sidebar, top to bottom: the Portraits panel
                        // (people, skin, eyes, background, `panels/portrait.rs`),
                        // then Normal's four groups (`panels/normal.rs` — the
                        // one-click look, fine-tune, noise, save), then the
                        // shared AI / Develop / Retouch panels.
                        self.portrait_panel(ui);
                        self.normal_panel(ui);
                        // The AI area is FIRST among the shared panels (R22 #4):
                        // one first-level section holding every AI develop verb
                        // — analysis, whole-image generation, reverse-fit —
                        // above the sliders it writes into. It used to be three
                        // fragments: a section inside Develop, a section in
                        // the middle of Retouch, and a switch in Settings.
                        self.ai_panel(ui);
                        self.develop_panel(ui);
                        self.retouch_panel(ui);
                    } else {
                        // Empty state with the same voice as the canvas hero:
                        // centered, quiet, and it says what to do next — a
                        // bare left-aligned label read as a stuck error.
                        ui.gap(24.0);
                        ui.vertical_centered(|ui| {
                            ui.label(Text::new("🖼").size(26.0).weak());
                            ui.gap(SPACE_SM);
                            ui.label(Text::new(tr(self.lang, K_APP_NO_PHOTO_OPEN)).weak());
                            ui.label(
                                Text::new(tr(
                                    self.lang,
                                    K_APP_CLICK_A_THUMBNAIL_IN_THE_LIBRARY_OR_PRESS_CTRL_O,
                                ))
                                .weak()
                                .small(),
                            );
                            ui.gap(SPACE_MD);
                            // Round-13 easter egg — quiet enough to miss,
                            // which is the point of an easter egg.
                            ui.label(
                                Text::new(tr(
                                    self.lang,
                                    K_APP_FUN_FACT_THIS_EMPTY_STATE_FINISHED_LOADING_WHILE,
                                ))
                                .weak()
                                .small()
                                .italics(),
                            );
                        });
                    }
                });
        });
    }

    pub(crate) fn upd_strips_and_side_panels(&mut self, ctx: &Context) {
        // Variant strip — sits directly above the status bar (registered after
        // it so it stacks on top), only when a photo is open. The selector for
        // 原片 / AI 生成 / 反推 renditions.
        if self.editor.photo().src_path.is_some() {
            TopBottomPanel::bottom("variants")
                // At the old 96 the cards' slack all pooled below the labels
                // — height and its derivation live on VARIANT_STRIP_H.
                .exact_height(AutoShadeApp::VARIANT_STRIP_H)
                .show(ctx, |ui| {
                    self.variant_strip(ui);
                });
        }

        // Left-most: the library gallery (folder browse + thumbnails), then
        // the develop controls. Tab collapses both for an edge-to-edge canvas
        // (LR's panel-hiding grammar); all state lives on, only layout skips.
        if !self.workspace.ui().panels_hidden {
            SidePanel::left("gallery").default_width(240.0).show(ctx, |ui| {
                self.with_panel(|view| view.gallery_panel(ui));
            });

            let controls = controls_side_panel(ctx, |ui| self.controls_column(ui));
            // ↑/↓ routing (see upd_shortcuts): over this panel the keys
            // belong to a hovered slider; anywhere else they walk the
            // library. One frame stale — panels draw after the shortcuts.
            self.workspace.view_mut().controls_rect = Some(controls.response.rect);
        } else {
            self.workspace.view_mut().controls_rect = None;
        }

        // Re-develop AFTER the controls are read (so this frame reflects edits).
        // The preview build runs on a SINGLE background worker (latest wins), so
        // egui's update loop never blocks on it — the old synchronous path froze
        // the UI for the whole develop (100-300 ms at 2560/4096, and 0.6-1.2 s
        // once a v0.8 zoned colour mask was present). While a frame is in flight,
        // further edits only set `dirty`; the completion handler discards a stale
        // frame and this block re-dispatches the newest recipe next tick, so
        // fast drags coalesce to the worker's throughput without a render storm.
        // The Before pane mirrors the canvas recipe's base_curve (calibration,
        // not an edit — the ● logic ignores it, but the compare must not):
    }

    /// One update() phase — body extracted verbatim from the eframe
    /// update loop (round-12 decomposition).
    pub(crate) fn upd_central(&mut self, ctx: &Context) {
        // The canvas sits on a bed one step below the panel fill (recessed
        // stage / raised chrome) — keep the stock margins, swap the fill.
        let bed = Frame::central_panel(&ctx.style())
            .fill(self.workspace.ui().theme.colors().canvas_bed);
        CentralPanel::default().frame(bed).show(ctx, |ui| {
            // Empty state: a real landing surface instead of a blank canvas.
            if self.editor.photo().src_path.is_none() {
                ui.vertical_centered(|ui| {
                    ui.gap(ui.available_height() * 0.32);
                    ui.heading("AutoShade");
                    ui.label(Text::new(tr(self.lang, K_APP_AI_AUTO_EDIT_RAW_PROCESSING)).weak());
                    ui.gap(SPACE_LG);
                    ui.row(|ui| {
                        // Center the button pair by padding half the leftover width.
                        let w = 300.0;
                        ui.gap((ui.avail_width() - w).max(0.0) * 0.5);
                        if primary(ui, true, tr(self.lang, K_APP_OPEN_PHOTO_CTRL_O)).clicked() {
                            self.apply_actions(vec![logic::Action::Gallery(
                                logic::gallery::GalleryAction::OpenPhotoFile,
                            )]);
                        }
                        if action(ui, true, tr(self.lang, K_APP_OPEN_FOLDER)).clicked() {
                            self.apply_actions(vec![logic::Action::Gallery(
                                logic::gallery::GalleryAction::OpenFolder,
                            )]);
                        }
                    });
                    ui.gap(8.0);
                    ui.label(
                        Text::new(tr(self.lang, K_APP_OR_DRAG_A_RAW_IMAGE_STRAIGHT_INTO_THE_WINDOW_DRA))
                            .weak()
                            .small(),
                    );
                });
                return;
            }

            // Fit BOTH dimensions (max_width alone lets a portrait overflow the
            // panel). The displayed rect is what paint/box-select map against,
            // so sizing here never changes their coordinate math.
            let avail = ui.available_size() - vec2(0.0, 22.0); // room for the caption row
            // Hold B to flash the source in place — the Lightroom compare
            // gesture (\ latches it until pressed again, D13). Focus-gated
            // like every other shortcut: typing a "b" into Direction must
            // not flash the Before mid-word.
            let comparing = self.workspace.view().before
                || (ctx.memory(|m| m.focused()).is_none()
                    && ctx.input(|i| Chord::B.held(i)));

            match self.workspace.view().mode {
                ViewMode::SideBySide => {
                    let half = (avail.x - 16.0) * 0.5;
                    let uv = self.editor.view_uv(self.workspace); // same window for both panes (synced zoom)
                    ui.row(|ui| {
                        ui.column(|ui| {
                            ui.label(
                                Text::new(tr(self.lang, K_APP_BEFORE_SOURCE))
                                    .weak()
                                    .small(),
                            );
                            if let Some(t) = &self.editor.plates().before_tex {
                                let size = t.size_vec2();
                                let vis = vec2(uv.width() * size.x, uv.height() * size.y);
                                let disp = fit_in(vis, half, avail.y);
                                let (rect, _) =
                                    ui.allocate_exact_size(disp, Sense::hover());
                                ui.painter_at(rect).image(t.id(), rect, uv, Color32::WHITE);
                            }
                        });
                        ui.rule();
                        ui.column(|ui| self.after_view(ui, half, avail.y, comparing));
                    });
                }
                ViewMode::AfterOnly => {
                    ui.column(|ui| self.after_view(ui, avail.x, avail.y, comparing));
                }
            }
        });
    }

    /// One update() phase — body extracted verbatim from the eframe
    /// update loop (round-12 decomposition).
    pub(crate) fn upd_sheets(&mut self, ctx: &Context) {
        self.analysis_full_window(ctx);
        self.recipe_library_window(ctx);
        // Settings window (provider / model / API keys). A local `open` avoids a
        // double &mut self borrow (Window::open vs the closure that reads self).
        if self.workspace.ui().show_settings {
            let mut open = true;
            // Scroll inside the window, capped below the screen height: the
            // provider sections outgrow a small display, and without a scroll
            // area the 保存 button ends up unreachable off-screen.
            let max_h = ctx.content_rect().height() * 0.85;
            Window::new(tr(self.lang, K_APP_SETTINGS))
                // Fixed id: the TITLE now varies with the language, and egui
                // keys window state (position, size) off the id.
                .id(Id::new("settings_window"))
                .collapsible(false)
                .resizable(false)
                .default_width(480.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ScrollArea::vertical()
                        .max_height(max_h)
                        .show(ui, |ui| self.with_panel(|view| view.settings_ui(ui)));
                });
            if !open {
                self.workspace.ui_mut().show_settings = false;
            }
        }

        // Keyboard cheat-sheet (F1 / ? / the ⌨ toolbar button) — the full
        // shortcut + gesture map lived only in tooltips and a code comment;
        // O (mask overlay) had no visible control at all.
        if self.workspace.ui().show_shortcuts {
            let mut open = true;
            let lang = self.lang;
            let accent = self.workspace.ui().theme.colors().accent_text;
            Window::new(tr(lang, K_APP_SHORTCUTS))
                // Fixed id, as for Settings: the title varies with the language
                // and egui keys window state (position) off the id.
                .id(Id::new("shortcuts_window"))
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    // Runtime table (not `const`): the ZH column is resolved by
                    // `tr` at draw time. ASCII key combos carry no
                    // natural-language words, so they stay literal.
                    let rows: [(&str, &str); 33] = [
                        ("Ctrl/⌘+O", tr(lang, K_APP_OPEN_PHOTO__2)),
                        ("Ctrl/⌘+Shift+E / Ctrl/⌘+E", tr(lang, K_APP_EXPORT_TO_THE_DESTINATION_DESTINATION_SETTINGS_I)),
                        ("Ctrl/⌘+S", tr(lang, K_APP_SAVE_DEVELOP_RECIPE_XMP_FOR_RAW)),
                        ("Ctrl/⌘+Z / +Shift+Z / +Y", tr(lang, K_APP_UNDO_REDO)),
                        ("Ctrl/⌘+Shift+C / +Shift+V", tr(lang, K_APP_COPY_RECIPE_PASTE_TO_SELECTED)),
                        ("← / →", tr(lang, K_APP_STEP_THROUGH_THE_LIBRARY)),
                        ("↑ / ↓", tr(lang, K_APP_STEP_THROUGH_THE_LIBRARY_OUTSIDE_THE_CONTROLS_PA)),
                        ("+ / − / 0 / 1", tr(lang, K_APP_ZOOM_IN_OUT_FIT_1_1)),
                        ("R", tr(lang, K_APP_ENTER_EXIT_CROP)),
                        ("Enter", tr(lang, K_APP_COMMIT_THE_CROP_EXIT_THE_TOOL)),
                        ("W", tr(lang, K_APP_WB_EYEDROPPER)),
                        ("Q", tr(lang, K_APP_RETOUCH_STAMP)),
                        ("K", tr(lang, K_APP_PAINT_MASK_BRUSH)),
                        ("M / Shift+M", tr(lang, K_APP_LINEAR_RADIAL_GRADIENT)),
                        (tr(lang, K_APP_SHIFT_WHILE_DRAWING_A_GRADIENT), tr(lang, K_APP_LOCK_TO_HORIZONTAL_VERTICAL)),
                        ("[ / ]", tr(lang, K_APP_BRUSH_SIZE_PAINT_CLONE_ARMED)),
                        ("Tab", tr(lang, K_APP_HIDE_SHOW_THE_SIDE_PANELS)),
                        (tr(lang, K_APP_HOVER_A_SLIDER), tr(lang, K_APP_NUDGE_ITS_VALUE_SHIFT_10)),
                        (tr(lang, K_APP_B_HOLD), tr(lang, K_APP_COMPARE_ORIGINAL)),
                        ("\\", tr(lang, K_APP_BEFORE_AFTER_TOGGLE)),
                        ("Y", tr(lang, K_APP_SIDE_BY_SIDE_SINGLE_VIEW)),
                        ("O", tr(lang, K_APP_TOGGLE_MASK_OVERLAY_CROP_CYCLE_GRID)),
                        ("J", tr(lang, K_APP_TOGGLE_CLIPPING_WARNING)),
                        ("Esc", tr(lang, K_APP_EXIT_TOOL_CLOSE_THIS_WINDOW)),
                        ("F1 / ?", tr(lang, K_APP_THIS_CHEAT_SHEET)),
                        (tr(lang, K_APP_SCROLL), tr(lang, K_APP_ZOOM_TOWARD_CURSOR)),
                        (tr(lang, K_APP_DOUBLE_CLICK_CANVAS), tr(lang, K_APP_FIT_1_1)),
                        (tr(lang, K_APP_SPACE_DRAG_MIDDLE_DRAG), tr(lang, K_APP_PAN)),
                        (tr(lang, K_APP_DRAG_WHEN_ZOOMED), tr(lang, K_APP_PAN_CTRL_DRAG_BOX_SELECT)),
                        (tr(lang, K_APP_ALT_CLICK), tr(lang, K_APP_SAMPLE_CLONE_SOURCE)),
                        (tr(lang, K_APP_SLIDER_DOUBLE_CLICK_RIGHT_CLICK), tr(lang, K_APP_RESET_TO_ITS_DEFAULT)),
                        (tr(lang, K_APP_CURVE_CLICK_DRAG_DRAG_OUT), tr(lang, K_APP_ADD_MOVE_DELETE_POINT)),
                        (tr(lang, K_APP_DRAG_A_MASK_HANDLE), tr(lang, K_APP_RESHAPE_MOVE_THE_SELECTED_MASK)),
                    ];
                    // Bounded + scrollable: the 30-row grid clipped its lower
                    // entries off short displays (the window is non-resizable
                    // and had no height limit, unlike the Settings window).
                    let max_h = (ctx.content_rect().height() * 0.7).max(200.0);
                    ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
                        Grid::new("shortcut_grid").num_columns(2).striped(true).show(
                            ui,
                            |ui| {
                                for (keys, what) in rows {
                                    ui.label(Text::new(keys).monospace().color(accent));
                                    ui.label(what);
                                    ui.end_row();
                                }
                            },
                        );
                    });
                    ui.gap(SPACE_SM);
                    // Round-13 easter egg: the pricing model, disclosed.
                    ui.label(
                        Text::new(tr(
                            lang,
                            K_APP_EVERY_SHORTCUT_ABOVE_SHIPS_FREE_NO_CREATIVE_TIER,
                        ))
                        .weak()
                        .small(),
                    );
                });
            if !open {
                self.workspace.ui_mut().show_shortcuts = false;
            }
        }
    }
}
