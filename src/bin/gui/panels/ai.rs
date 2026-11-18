//! AI panel: every AI *develop* verb and its output in ONE first-level area.
//!
//! R22 #4. Before this the AI surface was scattered across three panels: the
//! verdict + Direction + Analyze/Refine + Style lived at the top of Develop,
//! whole-image Reimagine and reverse-fit sat in the middle of Retouch (between
//! the brush tools), and reverse-fit's own 「Zoned fit (sky)」 switch was in
//! Settings, two panels away from the button it changes. One AI area —
//! analysis / reference libraries / whole-image generation / generated-image
//! adjustment / reverse-fit — is the whole idea; the panel sits at the TOP of the side
//! panel because it is the headline feature, above the sliders it writes into.
//!
//! What deliberately did NOT move: the PIXEL-level AI verbs (AI select
//! subject / sky in Local Masks, AI Denoise now in Detail, AI heal in Retouch,
//! export-time AI Denoise). Those belong beside the thing they act on; they
//! carry a 🤖 prefix and a cross-reference tooltip (`util::ai_xref`) instead,
//! and the index line at the foot of this panel says where each one is.
//!
//! R23 landed three more controls HERE (locked plan, docs/ROADMAP.md
//! 「第二十二至二十四轮计划」), all analysis-side and so all inside
//! `ai_analysis`, beside the Direction they steer: the style-library entry with
//! its reference-image switch (R23-2), the grade STRENGTH slider (R23-3, beside
//! Style — they are one pair of taste axes, and the reported defect was that
//! only the style half existed), and the 「Deep thinking」 switch (R23-4,
//! feedback #13), which sits under both dials because it reads them: the visual
//! judge's round budget comes off the Strength band directly above it.
//!
//! R30 cut the panel along the line the USER PAYS ON. Each sub-area is its
//! own `CollapsingHeader` whose title carries a COST TAG (`· paid API` /
//! `· local`). The reference library — local disk, CPU and one model download,
//! never an API call — is its own sub-area ([`AutoShadeApp::ai_libraries`]).
//! A library is read only while Style > 0 (`pipeline.rs`'s
//! `(req.style > 0.0).then(library::load)`), so the controls that only feed an
//! analysis go dark at Style 0 while the build side stays live: building a
//! library is not reading one, and greying the build at Style 0 would make the
//! library nobody has yet the one nobody can make.
use crate::*;

impl Commands<'_> {

    /// The AI area — TWO first-level sections (user ruling 2026-10-20: the two
    /// AI paths read as one 「AI」 heap before, and they are different
    /// products): 「AI generation & reverse-fit」 — an image model paints the
    /// look, the fit recovers it as a recipe — and 「AI analysis & auto
    /// develop」 — a vision model proposes or adjusts the recipe directly.
    /// Each is a panel title like Develop's, with its own fold state.
    ///
    /// The editable gate is REBUILT here (L15-2). While a decode is in flight
    /// (`open_in_flight`) these controls would read and write the STASHED photo
    /// A while B lands and replaces the whole recipe — silent input loss. The
    /// analysis half used to inherit `develop_panel`'s `add_enabled_ui`
    /// wrapper; moving it out of that closure without restoring the same gate
    /// here would have re-opened exactly that hole. `busy` alone must NOT gate:
    /// a 600 s analyze keeps the panel live (each verb has its own `ready`
    /// gate) — only the open transition freezes it.
    pub(crate) fn ai_panel(&mut self, ui: &mut Ui) {
        let lang = self.lang; // Copy — never borrows self, safe inside egui closures.
        let editable = !self.workspace.open().open_in_flight;
        // The gate wraps BOTH areas, headers included — exactly how
        // develop_panel wraps its sections, so the three read the same mid-open
        // (greyed) instead of one panel looking live beside a dead one.
        ui.enabled(editable, |ui| {
            // ① Generation: the ● means this photo has a generated card in
            // play (the prompt is a remembered preference, not photo state —
            // see `ai_section_active`).
            let generated = self.editor.active_on_ai_pixels();
            self.ai_section(ui, "sec_ai_gen", tr(lang, K_AI_AI_GENERATION_REVERSE_FIT), generated, |app, ui| {
                app.ai_generate(ui);
                app.ai_adjust(ui);
                app.ai_reverse_fit(ui);
            });
            ui.gap(SPACE_MD);
            // ② Analysis: the ● is a verdict or a Direction. Same persistent id
            // as the single heading it grew out of, so an open state a user
            // already has carries over.
            let analysed = self.editor.ai_section_active(&self.workspace);
            self.ai_section(ui, "sec_ai", tr(lang, K_AI_AI_ANALYSIS_AUTO_DEVELOP), analysed, |app, ui| {
                #[cfg(test)]
                {
                    // The gate's own witness: a comment cannot keep it here.
                    app.seams.ai_gate_enabled = Some(ui.is_enabled());
                }
                app.ai_analysis(ui);
                app.ai_adjust_edit(ui);
                app.ai_libraries(ui);
                // Where the AI verbs that did NOT move to this panel live.
                // A panel called "AI" reads as the complete inventory
                // otherwise, and the pixel-level tools are deliberately
                // elsewhere.
                ui.gap(SPACE_XS);
                explainer(ui, lang, tr(lang, K_AI_PIXEL_LEVEL_AI_TOOLS_STAY_AT_THEIR_TOOLS_SELECT));
            });
        });
        ui.gap(SPACE_MD); // fence to the Develop heading below
    }

    /// One first-level AI section: a panel title like Develop's and
    /// Retouch's (`panel_heading`), only collapsible. `CollapsingHeader`
    /// indents its body, which set every AI row 18 px right of the other two
    /// panels' rows and cut its cells by 9 px (2026-09-27): the body is laid
    /// UNINDENTED, and the folds inside keep their own indent, so their rows
    /// start where a Develop fold's do. The title is a Heading (2026-10-01:
    /// drawn at fold size, the folds under it read as its siblings).
    fn ai_section(
        &mut self,
        ui: &mut Ui,
        id: &str,
        title: &str,
        active: bool,
        body: impl FnOnce(&mut Self, &mut Ui),
    ) {
        let mut state = collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            ui.make_persistent_id(id),
            true,
        );
        let header = panel_heading(ui, title, active, |ui| {
            state.show_toggle_button(ui, paint_chevron);
        })
        .on_hover_cursor(CursorIcon::PointingHand);
        if header.clicked() {
            state.toggle(ui);
        }
        state.show_body_unindented(ui, |ui| body(self, ui));
        state.store(ui.ctx());
    }




    /// The analysis output's full text in a window of its own: movable,
    /// resizable, scrolling, its text selectable. Not a transient sheet — it
    /// is a reading pane, so the editor's keys stay live beside it. Closes
    /// itself once there is nothing to show (a new photo, a cleared result).
    pub(crate) fn analysis_full_window(&mut self, ctx: &Context) {
        if !self.workspace.ui().show_analysis_full {
            return;
        }
        let mut open = self.editor.has_analysis_output();
        Window::new(tr(self.lang, K_AI_AI_ANALYSIS_FULL_TEXT))
            // Fixed id: the title varies with the language, and egui keys
            // the window's position and size off the id.
            .id(Id::new("analysis_full_window"))
            .collapsible(false)
            .resizable(true)
            .default_size(vec2(520.0, 420.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.with_panel(|p| p.analysis_output(ui)));
            });
        self.workspace.ui_mut().show_analysis_full = open;
    }

    /// Sub-area ① — ANALYSIS (paid API): the verdict and rationale one Analyze
    /// run wrote, the Direction prompt it reads, its two verbs, and the two
    /// taste dials that steer them. Body migrated verbatim from
    /// `develop::dev_ai` (R22 #4); R30 moved the LIBRARY half out of it into
    /// [`AutoShadeApp::ai_libraries`], so this fold is now exactly the controls
    /// that spend money — which is what its 「· paid API」 tag claims while it
    /// is collapsed.
    ///
    /// Default OPEN: it is the panel's first read.
    fn ai_analysis(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        section_fold(ui, "sec_ai_analysis", tr(lang, K_AI_ANALYSIS_PAID_API), true, true, |ui| {
            // The analysis output lives in ONE bounded box (2026-10-13, user:
            // 「思考过程输出有时候太长了会占满屏幕」): the verdict, the rationale
            // sentence and the deep-thinking working scroll inside
            // ANALYSIS_BOX_MAX_H, and once they overflow it a verb opens the
            // whole text in a window of its own (`analysis_full_window`).
            if self.editor.has_analysis_output() {
                let out = Frame::group(ui.style())
                    .show(ui, |ui| {
                        ScrollArea::vertical()
                            .id_salt("ai_analysis_box")
                            .max_height(ANALYSIS_BOX_MAX_H)
                            .auto_shrink([false, true])
                            .show(ui, |ui| self.with_panel(|p| p.analysis_output(ui)))
                    })
                    .inner;
                #[cfg(test)]
                {
                    self.seams.analysis_box_rect = Some(out.inner_rect);
                }
                if out.content_size.y > out.inner_rect.height() + 0.5
                    && action_row(ui, true, tr(lang, K_AI_VIEW_FULL_TEXT)).clicked()
                {
                    self.workspace.ui_mut().show_analysis_full = true;
                }
            }
            // Library OR direction (user ruling 2026-10-14): the switch heads
            // the controls it chooses between, and greys the other side.
            use autoshade::pipeline::Guide;
            ui.row_wrapped(|ui| {
                ui.label(tr(lang, K_AI_GUIDED_BY));
                ui.radio_value(&mut self.aip.guide, Guide::Library, tr(lang, K_AI_REFERENCE_LIBRARY)).on_hover_text(tr(
                    lang,
                    K_AI_THE_AI_FOLLOWS_THE_FINISHED_PHOTOS_IN_YOUR_REFER,
                ));
                ui.radio_value(&mut self.aip.guide, Guide::Direction, tr(lang, K_AI_DIRECTION)).on_hover_text(tr(
                    lang,
                    K_AI_THE_AI_FOLLOWS_THE_DIRECTION_YOU_TYPE_THE_REFERE,
                ));
            });
            let by_direction = self.aip.guide == Guide::Direction;
            ui.enabled(by_direction, |ui| {
                // #14a: was `desired_width(f32::INFINITY)` — a full-panel ribbon on a
                // wide side panel. `prompt_field` caps it, widens while focused and
                // shows the whole text on hover.
                let _field = prompt_field(
                    ui,
                    &mut self.workspace.ai_mut().guidance,
                    tr(lang, K_AI_E_G_WARMER_AND_MOODIER_LIFT_THE_SHADOWS),
                );
                #[cfg(test)]
                {
                    self.seams.prompt_rects.push(_field.rect);
                }
            });
            // The prompt's trigger sits DIRECTLY under it (user feedback:
            // the toolbar Analyze button sat nowhere near the text it
            // consumes). The old 「AI Refine」 beside it is gone (user ruling
            // 2026-10-20): adjusting the edit you are looking at has its own
            // fold with its own request box — `ai_adjust_edit` below.
            //
            // `analyze_inflight` too, or ✕ leaves this ENABLED while
            // `logic::look::plan` silently refuses (it must refuse — the
            // cancelled call is still on the wire and still billing).
            // A button that looks live and does nothing, for up to the
            // 600 s stall budget, with the status line saying the app
            // is free, is worse than one that says why it is greyed.
            let ready = self.editor.photo().src_path.is_some() && !self.workspace.busy() && !self.workspace.tools().analyze_inflight;
            let waiting = self.workspace.tools().analyze_inflight && !self.workspace.busy();
            let why = |ui: Response| {
                if waiting {
                    ui.on_hover_text(tr(
                        lang,
                        K_AI_THE_CANCELLED_AI_CALL_IS_STILL_RUNNING_AND_STILL,
                    ))
                } else {
                    ui
                }
            };
            if why(primary_row(ui, ready, tr(lang, K_AI_AI_ANALYZE))
                .on_hover_text(tr(lang,
                    K_AI_AI_PROPOSES_A_RECIPE_FROM_SCRATCH_THE_CONFIGURED,
                )))
                .clicked()
            {
                let mut intent = logic::look::LookIntent::of(logic::look::LookKind::Analyze);
                intent.typed = self.workspace.ai().guidance.clone();
                intent.region = self.workspace.overlay().region;
                self.apply_actions(vec![logic::Action::Look(intent)]);
            }
            // The two TASTE DIALS get their own lines, below the verbs (R23-3).
            //
            // They used to share the verbs' row, with a note claiming the wrap kept
            // Style on-panel at narrow widths. It did not: `Slider` lays its
            // value box, track and LABEL out in a nested `ui.horizontal`, and a
            // nested row never wraps in a wrapping parent — it overflows and is
            // clipped. At the default 320 px side panel the row drew only Style's
            // value ("30"), with the word "Style" itself off-panel, and a headless
            // frame confirms it. Adding a second dial to that row hid it outright.
            // One dial per line is also what every other slider in this app does.
            ui.enabled(!by_direction, |ui| ui.row_wrapped(|ui| {
                #[cfg(test)]
                {
                    self.seams.guide_gate_enabled = Some((by_direction, ui.is_enabled()));
                }
                // #16: the ONE slider in the app that never went through the
                // panel's own helper — a bare `Slider` with `show_value(false)`
                // and a hand-rolled "30%" label beside it, so it alone had no
                // double-click / right-click reset and no hover ↑/↓ nudge, and its
                // 0..1 storage was shown on a scale nothing else in the UI uses.
                // `slider_pct_hinted` puts it on the same 0..100 track as every
                // other stored fraction (Amount, feathers, tolerance), resets to the
                // shared default, and keeps the explanation as the tooltip's first
                // line instead of on a separate label.
                slider_pct_hinted(
                    ui,
                    lang,
                    tr(lang, K_AI_STYLE),
                    &mut self.aip.style_strength,
                    1.0,
                    STYLE_STRENGTH_DEFAULT,
                    tr(lang, K_AI_HOW_FAR_AI_PROPOSALS_FOLLOW_THE_LOOKS_IN_YOUR_RE),
                );
                // R23-2: a slider that provably cannot do anything must not read as
                // live. The old one showed 30% and a tooltip about "your past XMP
                // editing habits" on a fresh install with no library at all — a
                // control that was permanently inert with nothing saying so. Only
                // when the status is KNOWN and negative (never while it is still
                // being read, which would flash a false warning every launch).
                if matches!(
                    self.workspace.ai().style_info,
                    Some(autoshade::library::Summary::Absent | autoshade::library::Summary::Unusable(_))
                ) {
                    ui.label(
                        Text::new(tr(lang, K_AI_NO_LIBRARY))
                            .color(ui.visuals().warn_fg_color)
                            .small(),
                    )
                    .on_hover_text(tr(lang,
                        K_AI_THIS_SLIDER_DOES_NOTHING_UNTIL_A_REFERENCE_LIBRA,
                    ));
                }
            }));
            // R23-3 (feedback #5): the SECOND taste axis, directly under the first —
            // they are a PAIR, and the whole reported problem was that only the style
            // half existed, so "lean on my habits" was the only dial there was and it
            // bought MORE restraint. Same helper as Style, so it gets the same 0..100
            // track, double-click reset and ↑/↓ nudge.
            slider_pct_hinted(
                ui,
                lang,
                tr(lang, K_AI_STRENGTH),
                &mut self.aip.grade_strength,
                1.0,
                GRADE_STRENGTH_DEFAULT,
                tr(lang,
                    K_AI_HOW_BOLD_THE_AI_S_EDIT_IS_A_DIFFERENT_AXIS_FROM,
                ),
            );
            let has_direction = by_direction && !self.workspace.ai().guidance.trim().is_empty();
            ui.enabled(has_direction, |ui| {
                #[cfg(test)]
                {
                    // The gate's own witness (same seam as `ai_gate_enabled`): a
                    // comment cannot keep the wrapper here.
                    self.seams.adherence_gate_enabled = Some(ui.is_enabled());
                }
                slider_pct_hinted(
                    ui,
                    lang,
                    tr(lang, K_AI_ADHERENCE),
                    &mut self.aip.direction_adherence,
                    1.0,
                    autoshade::recipe::DirectionAdherence::DEFAULT,
                    tr(lang, K_AI_HOW_CLOSELY_THE_AI_FOLLOWS_YOUR_DIRECTION_DISABL),
                );
            });
            // R23-4 (feedback #13): the THIRD analysis-side control, under the two
            // dials it modifies — the target score and round budget it unlocks are
            // read off the Strength band directly above it.
            ui.checkbox(&mut self.aip.deep_think, tr(lang, K_AI_DEEP_THINKING))
                .on_hover_text(tr(lang,
                    K_AI_MAKE_THE_AI_SHOW_ITS_WORK_AND_LET_IT_ITERATE_THE,
                ));
            });
    }

    /// Sub-area ①b — ADJUST THE SELECTED VARIANT (paid API; user ruling
    /// 2026-10-20, replacing 「AI Refine」). The request box and its verb,
    /// together: the AI is shown the selected card AS IT LOOKS NOW and its
    /// recipe, and returns the complete recipe that replaces it — every
    /// develop control, and parts of the picture selected by content through
    /// masks the local segmenter cuts (`pipeline::adjust_envelope`,
    /// `segment::resolve_ai_masks`). Lands like an analysis: into the sliders,
    /// undoable, and on a new ✎ card when the selected card is the pristine ✨.
    fn ai_adjust_edit(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        ui.gap(SPACE_XS);
        section_fold(ui, "sec_ai_adjust_edit", tr(lang, K_AI_ADJUST_THE_SELECTED_VARIANT_PAID_API), true, true, |ui| {
                explainer(ui, lang, tr(lang, K_AI_TYPE_WHAT_TO_CHANGE_ON_THE_SELECTED_CARD_THE_AI));
                let _field = prompt_field(
                    ui,
                    &mut self.workspace.genai_mut().adjust_edit_prompt,
                    tr(lang, K_AI_E_G_DARKEN_THE_SKY_A_LITTLE_WARM_THE_FACES_MORE),
                );
                #[cfg(test)]
                {
                    self.seams.prompt_rects.push(_field.rect);
                }
                let ready = self.editor.photo().src_path.is_some() && !self.workspace.busy() && !self.workspace.tools().analyze_inflight;
                let asked = ready && !self.workspace.genai().adjust_edit_prompt.trim().is_empty();
                let resp = primary_row(ui, asked, tr(lang, K_AI_AI_ADJUST))
                    .on_hover_text(tr(lang,
                        K_AI_ADJUST_THE_SELECTED_CARD_S_DEVELOP_AS_YOU_ASK_TH,
                    ));
                #[cfg(test)]
                {
                    self.seams.adjust_edit_btn_enabled = Some(resp.enabled());
                }
                if resp.clicked() {
                    let mut intent = logic::look::LookIntent::of(logic::look::LookKind::Adjust);
                    intent.typed = self.workspace.genai().adjust_edit_prompt.clone();
                    intent.region = self.workspace.overlay().region;
                    intent.base = Some(self.editor.doc().recipe.clone());
                    intent.pixels = self.editor.active_on_ai_pixels()
                        .then(|| self.editor.active_variant().and_then(|v| v.origin.clone()))
                        .flatten();
                    self.apply_actions(vec![logic::Action::Look(intent)]);
                }
            });
    }

    /// Sub-area ② — the REFERENCE LIBRARY (local): finished photos whose look
    /// an analysis may follow — which library, how to build it, and the two
    /// switches an analysis reads.
    ///
    /// COLLAPSED by default because it is a setup-time surface: built once,
    /// then left alone. The status line is half the feature (R23-2: "I have no
    /// idea which library it is referencing"), so it heads the fold.
    fn ai_libraries(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        // Read the status ONCE per session (cached in `style_info`). OUTSIDE
        // the fold on purpose: the Style slider's own 「⚠ no library」 flag one
        // sub-area up reads this same answer, so a collapsed fold must not
        // starve it. Not in tests: a headless frame would spawn a worker
        // reading the developer's own store; tests set `style_info` directly.
        if self.workspace.ai().style_info.is_none() && !self.workspace.ai().style_info_loading {
            self.apply_actions(vec![logic::Action::Ai(logic::ai::AiAction::RefreshLibraryStatus)]);
        }
        ui.gap(SPACE_XS);
        // Read off the SAME comparison the pipeline makes
        // (`GradeRequest::library_style`), so the two can never disagree
        // about what "reads a library" means.
        let by_direction = self.aip.guide == autoshade::pipeline::Guide::Direction;
        let reads_a_library = !by_direction && self.aip.style_strength > 0.0;
        section_fold(ui, "sec_ai_libraries", tr(lang, K_AI_REFERENCE_LIBRARY_LOCAL), false, true, |ui| {
                self.with_panel(|p| p.ai_library_status(ui));
                self.ai_library_build(ui);
                if !reads_a_library {
                    // Outside the disabled scope below: the sentence that
                    // explains a greyed control must not itself be greyed.
                    let why = if by_direction {
                        tr(lang, K_AI_GUIDED_BY_THE_DIRECTION_THE_ANALYSIS_READS_NO_LI)
                    } else {
                        tr(lang, K_AI_STYLE_IS_AT_0_THE_ANALYSIS_READS_NO_LIBRARY)
                    };
                    ui.label(Text::new(why).weak().small());
                }
                ui.enabled(reads_a_library, |ui| {
                    #[cfg(test)]
                    {
                        self.seams.ai_library_read_enabled = Some(ui.is_enabled());
                    }
                    self.with_panel(|p| p.ai_library_use(ui));
                });
            });
    }


    /// The folder picker, the build button, the phrase switch and progress —
    /// the BUILD side, live at every Style value; only a running build greys it.
    fn ai_library_build(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        let building = self.workspace.ai().style_build_inflight;
        let mut pick = false;
        let mut build: Option<PathBuf> = None;
        ui.enabled(!building, |ui| {
            #[cfg(test)]
            {
                self.seams.ai_library_build_enabled = Some(ui.is_enabled());
            }
            // 🗂, the same glyph Settings uses for a folder action: the
            // embedded font subset covers it already (📂 is not in it).
            let cell = columns(ui, 2);
            ui.row(|ui| {
            if action_in(ui, cell, true, tr(lang, K_AI_PICK_FOLDER))
                .on_hover_text(tr(lang,
                    K_AI_CHOOSE_A_FOLDER_OF_FINISHED_PHOTOS_WHOSE_LOOK_YO,
                ))
                .clicked()
            {
                pick = true;
            }
            // Rebuilding needs a folder: the remembered one, or one picked now.
            let have = self.aip.style_src_dir.clone().filter(|d| d.is_dir());
            let label = if building { tr(lang, K_AI_BUILDING) } else { tr(lang, K_AI_BUILD_REBUILD) };
            let resp = primary_in(ui, cell, have.is_some(), label).on_hover_text(if have.is_some() {
                tr(lang,
                    K_AI_MEASURE_EVERY_FINISHED_PHOTO_IN_THAT_FOLDER_LOCA,
                )
            } else {
                tr(lang, K_AI_PICK_A_FOLDER_FIRST)
            });
            if resp.clicked() {
                build = have;
            }
            });
            // The RAW folder for the settings check: optional, remembered, and
            // read on the next build.
            let mut pick_raw = false;
            ui.row(|ui| {
            if action_in(ui, cell, true, tr(lang, K_AI_RAW_FOLDER))
                .on_hover_text(tr(lang,
                    K_AI_OPTIONAL_THE_FOLDER_YOUR_RAWS_LIVE_IN_ON_THE_NEX,
                ))
                .clicked()
            {
                pick_raw = true;
            }
            if action_in(ui, cell, self.aip.style_raw_dir.is_some(), tr(lang, K_AI_CLEAR))
                .on_hover_text(tr(lang, K_AI_STOP_LOOKING_FOR_RAW_SETTINGS))
                .clicked()
            {
                self.aip.style_raw_dir = None;
            }
            });
            let raw_note = match &self.aip.style_raw_dir {
                Some(d) => abs_display(d),
                None => tr(lang, K_AI_NO_RAW_FOLDER_REFERENCES_GO_WITHOUT_SETTINGS).to_string(),
            };
            ui.label(Text::new(raw_note).small().weak());
            if pick_raw {
                self.apply_actions(vec![logic::Action::Ai(logic::ai::AiAction::PickRawFolder)]);
            }
            ui.checkbox(&mut self.aip.style_describe, tr(lang, K_AI_WRITE_LOOK_PHRASES_LOCAL_MODEL))
                .on_hover_text(tr(lang,
                    K_AI_A_LOCAL_MODEL_QWEN3_VL_2B_4_3_GB_DOWNLOADED_ONCE,
                ));
        });
        if let Some((stage, done, total)) = self.workspace.ai().style_build_progress {
            ui.label(
                Text::new(trf(
                    lang,
                    K_AI_PHOTOS,
                    &[("stage", tr(lang, stage.label())), ("done", &done.to_string()), ("total", &total.to_string())],
                ))
                .weak()
                .small(),
            );
        }
        // The dialog is modal and blocks: the runtime opens it after the row
        // is laid out (it remembers the pick before starting the build).
        if pick {
            self.apply_actions(vec![logic::Action::Ai(logic::ai::AiAction::PickReferenceFolder)]);
        }
        if build.is_some() {
            self.apply_actions(vec![logic::Action::Ai(logic::ai::AiAction::BuildReferenceLibrary)]);
        }
    }


    /// Sub-area ③ — WHOLE-IMAGE GENERATION (paid API): let gpt-image DIRECTLY produce the
    /// picture (the optional "GPT makes the image" path). Distinct from AI
    /// Analyze, which emits a faithful parametric recipe. The result becomes a
    /// new「AI 生成」variant in the strip below; the reverse-fit sub-area under
    /// this one then closes the loop, adding a「反推」variant whose look lives
    /// in an editable recipe (full-res + XMP). No more "continue from master"
    /// button — each result is its own selectable variant, so a slider edit can
    /// never revert or double-cook it.
    ///
    /// Collapsed by default: this is the OPTIONAL path (and the paid one), so
    /// the analysis sub-area at the top stays the panel's first read. R30 put
    /// the cost in the header text as well — a collapsed fold that says
    /// 「· paid API」 is the only warning a user gets before opening it.
    fn ai_generate(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        ui.gap(SPACE_XS);
        section_fold(ui, "sec_ai_reimagine", tr(lang, K_AI_GENERATE_WHOLE_IMAGE_PAID_API), false, true, |ui| {
                // This entry's OWN style prompt, right next to its trigger
                // (it used to silently borrow the Direction field at the top
                // of the panel — a prompt and its button belong together).
                ui.row_wrapped(|ui| {
                    // The prompt field's width reserve follows the BUTTON'S
                    // localized label, measured — the old fixed 130 px was
                    // one pixel SHORT of the English "✨ Generate image"
                    // (131 px at Button style + padding; the zh label needs
                    // only 103), so the button's text wrapped into two
                    // lines, twice the height of every neighbour (R19 user
                    // report). Extend-mode is the belt to the measured
                    // braces: the button can never wrap internally again —
                    // which is also why the row math must be EXACT, frame
                    // margins included: with Extend, any surplus stops
                    // being absorbed by a wrap and instead widens the
                    // auto-fitting side panel by that surplus EVERY frame
                    // (probed: +8 px/frame runaway when the TextEdit's own
                    // frame margin was left out of the reserve). The margin
                    // is therefore pinned HERE and handed to the widget, so
                    // the arithmetic and the widget can never disagree.
                    const TE_MARGIN: Margin = Margin::symmetric(4, 2);
                    let btn_label = tr(lang, K_AI_GENERATE_IMAGE);
                    let btn_w = ui
                        .painter()
                        .layout_no_wrap(
                            btn_label.to_owned(),
                            TextStyle::Button.resolve(ui.style()),
                            ui.visuals().text_color(),
                        )
                        .size()
                        .x
                        + 2.0 * ui.spacing().button_padding.x;
                    // …and #14a caps the RESULT at FIELD_W_MAX, which only stops
                    // an 800 px panel from drawing an 800 px prompt ribbon —
                    // clamping DOWNWARD cannot feed the runaway above.
                    //
                    // The FLOOR is the honest half: FIELD_W_MIN does ask for
                    // more than a very narrow row has, the shape the runaway
                    // came from. It cannot self-feed the way the runaway did,
                    // because the demand is a CONSTANT 80 px rather than one
                    // defined in terms of `available_width()` — the panel widens
                    // once to fit it and then the surplus is gone. Behaviour is
                    // unchanged from R19 either way: FIELD_W_MIN IS that row's
                    // own `.max(80.0)`, now a named token (theme.rs). Both are
                    // consts with MIN < MAX, so `clamp` cannot hit its panicking
                    // case.
                    // The ceiling applies to the field AS DRAWN — the widget
                    // allocates `desired_width` PLUS its own margin, so the
                    // reserve above is subtracted from the drawn width, never
                    // asked for on top of it.
                    let field_drawn = (ui.avail_width()
                        - btn_w
                        - ui.spacing().item_spacing.x)
                        .clamp(FIELD_W_MIN, FIELD_W_MAX);
                    let field_w = field_drawn - TE_MARGIN.sum().x;
                    let _field = ui.add(
                        TextEdit::singleline(&mut self.workspace.genai_mut().reimagine_prompt)
                            .margin(TE_MARGIN)
                            .desired_width(field_w)
                            .hint_text(tr(lang, K_AI_STYLE_TO_REPAINT_TOWARD_E_G_GOLDEN_HOUR_GLOW_MOO)),
                    );
                    #[cfg(test)]
                    {
                        self.seams.prompt_rects.push(_field.rect);
                    }
                    ui.enabled(!self.workspace.busy(), |ui| {
                        let resp = ui.add(primary_button(btn_label).wrap_mode(TextWrapMode::Extend));
                        #[cfg(test)]
                        {
                            self.seams.reimagine_btn_rect = Some(resp.rect);
                        }
                        if resp
                            .on_hover_text(tr(lang,
                                K_AI_REPAINT_THE_WHOLE_IMAGE_WITH_GPT_IMAGE_STYLED_BY,
                            ))
                            .clicked()
                        {
                            self.apply_actions(vec![logic::Action::Ai(logic::ai::AiAction::Reimagine)]);
                        }
                    });
                });
                // Its OWN row, deliberately: a fourth item in the wrapped
                // prompt+button row re-opens the R19 width arithmetic.
                ui.checkbox(
                    &mut self.workspace.fit_mut().reimagine_retry,
                    tr(lang, K_AI_AUTO_RETRY_ONCE_IF_THE_RESULT_DIVERGES),
                )
                .on_hover_text(trf(
                    lang,
                    K_AI_AFTER_GENERATING_THE_STRUCTURAL_DIVERGENCE_D_VS,
                    &[("limit", &format!("{:.2}", autoshade::fit::DIVERGENCE_GLOBAL))],
                ));
                explainer(ui, lang, tr(lang, K_AI_AFTER_GENERATING_USE_REVERSE_FIT_RECIPE_TO_TURN));
            });
    }


    fn ai_adjust(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        ui.gap(SPACE_XS);
        section_fold(ui, "sec_ai_adjust", tr(lang, K_AI_ADJUST_GENERATED_IMAGE_PAID_API), false, true, |ui| {
                let _field = prompt_field(
                    ui,
                    &mut self.workspace.genai_mut().adjust_prompt,
                    tr(lang, K_AI_WHAT_TO_CHANGE_E_G_MAKE_THE_SKY_BLUER_REMOVE_THE),
                );
                #[cfg(test)]
                {
                    self.seams.prompt_rects.push(_field.rect);
                }
                ui.menu("adjust_quality", tr(lang, ["high", "medium", "low"][self.workspace.genai().adjust_quality.min(2)]), None, |ui| {
                        ui.selectable_value(&mut self.workspace.genai_mut().adjust_quality, 0, tr(lang, K_AI_HIGH));
                        ui.selectable_value(&mut self.workspace.genai_mut().adjust_quality, 1, tr(lang, K_AI_MEDIUM));
                        ui.selectable_value(&mut self.workspace.genai_mut().adjust_quality, 2, tr(lang, K_AI_LOW));
                    })
                    .on_hover_text(tr(lang, K_AI_GPT_IMAGE_RENDER_QUALITY_HIGHER_LOOKS_BETTER_AND));
                // This fold's own brush and painted area (2026-09-27): it used
                // to read the Retouch panel's shared mask with no control here.
                self.brush_row(ui, BrushOwner::Adjust);
                let note = if !self.editor.active_on_ai_pixels() {
                    tr(lang, K_AI_SELECT_A_AI_GENERATED_CARD_OR_ITS_EDIT_FIRST)
                } else if self.workspace.area_painted(BrushOwner::Adjust) {
                    tr(lang, K_AI_PAINTED_AREA_ONLY)
                } else {
                    tr(lang, K_AI_WHOLE_IMAGE_PAINT_AN_AREA_TO_LIMIT_IT)
                };
                ui.label(Text::new(note).weak().small());
                let resp = primary_row(ui, self.editor.can_adjust(&self.workspace, ), tr(lang, K_AI_ADJUST))
                    .on_hover_text(tr(lang,
                        K_AI_CHANGE_THE_GENERATED_PICTURE_WITH_THE_PROMPT_THE,
                    ));
                #[cfg(test)]
                {
                    self.seams.adjust_btn_enabled = Some(resp.enabled());
                }
                if resp.clicked() {
                    self.apply_actions(vec![logic::Action::Ai(logic::ai::AiAction::AdjustGenerated)]);
                }
            });
    }


    fn ai_reverse_fit(&mut self, ui: &mut Ui) {
        let lang = self.lang;
        ui.gap(SPACE_XS);
        section_fold(ui, "sec_ai_reverse_fit", tr(lang, K_AI_MATCH_A_LOOK_REVERSE_FIT_LOCAL), true, true, |ui| {
            let can_fit = self.editor.fit_target(&self.workspace).is_some() && self.editor.photo().source_preview.is_some();
            if !can_fit {
                // An ✎ card is not a fit target (`fit_target`): the fit reads
                // pixels, and the card's edits are sliders the user already
                // has — the line says which card to select instead of
                // pretending nothing was generated.
                let on_edited =
                    self.editor.active_variant().is_some_and(|v| v.kind == VariantKind::Edited);
                let note = if on_edited {
                    tr(lang,
                        K_AI_REVERSE_FIT_READS_THE_AI_GENERATED_CARD_S_PIXELS)
                } else {
                    tr(lang,
                        K_AI_PICK_A_REFERENCE_BELOW_OR_GENERATE_AN_IMAGE_AND)
                };
                ui.label(Text::new(note).weak().small());
            }
            // ── the SOURCE row (2026-09-30, user decision): which negative the
            // fit solves FROM — the ▣ card or any ◈ / ▦ master — named and
            // choosable. Until then the fold showed the target only, and the
            // source was a rule nobody could see (`negative_index`).
            let (auto, sources) = self.fit_source_menu();
            // The label needs the command context, which borrows the editor —
            // read the active index first, then render the label.
            let active = self.editor.doc().active;
            let card_label = self.numbered_card_label(active);
            let to = (self.workspace.fit().fit_ref.is_none() && self.editor.fit_target(&self.workspace).is_some())
                .then(|| trf(lang, K_ACTIONS_TO, &[("card", &card_label)]));
            if !sources.is_empty() {
                let shown = match self.workspace.fit().fit_from.as_deref() {
                    Some(id) => sources.iter().find(|(s, _)| s == id).map(|(_, l)| l.clone()),
                    None => None,
                }
                .unwrap_or_else(|| auto.clone());
                let mut pick: Option<Option<String>> = None;
                let busy = self.workspace.busy();
                ui.row(|ui| {
                    // The row's width is measured at its start, before the
                    // label, and capped at FIELD_W_MAX (`columns`), so the menu
                    // ends on the same right edge as the buttons under it;
                    // `available_width()` alone stretched it across a wide
                    // panel (2026-10-01 user report).
                    let row = columns(ui, 1).x;
                    let label = ui.label(tr(lang, K_ACTIONS_FROM)).rect.width();
                    let menu = (row - label - ui.spacing().item_spacing.x).max(FIELD_W_MIN);
                    let _menu = ui.enabled(!busy, |ui| {
                        ComboBox::from_id_salt("fit_from")
                            .width(menu)
                            .truncate()
                            .selected_text(shown)
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(self.workspace.fit().fit_from.is_none(), &auto).clicked() {
                                    pick = Some(None);
                                }
                                for (id, label) in &sources {
                                    let on = self.workspace.fit().fit_from.as_deref() == Some(id.as_str());
                                    if ui.selectable_label(on, label).clicked() {
                                        pick = Some(Some(id.clone()));
                                    }
                                }
                            })
                            .response
                            .on_hover_text(tr(lang,
                                K_AI_THE_NEGATIVE_THE_REVERSE_FIT_STARTS_FROM_THE_ORI,
                            ))
                    });
                    #[cfg(test)]
                    {
                        self.seams.fit_from_rect = Some(_menu.inner.rect);
                    }
                });
                if let Some(p) = pick {
                    self.workspace.fit_mut().fit_from = p;
                }
            }
            // ── the TARGET, named when it is the active card (a picked
            // reference file names itself in the row below).
            if let Some(to) = to {
                ui.label(Text::new(to).weak().small());
            }
            // ── the REFERENCE row (R23-6 B): any finished rendition of this same
            // frame — your own Lightroom export, the camera's JPEG, another RAW
            // developed elsewhere. The generated-variant entry below still works
            // untouched; this one simply stops it from being the only one.
            let mut pick_ref = false;
            let mut clear_ref = false;
            ui.row_wrapped(|ui| {
                // Two cells of the grid, measured at the row's start: the
                // verb, then the file name with its ✕ (2026-09-27, 2026-10-14).
                let cell = columns(ui, 2);
                if action_in(ui, cell, !self.workspace.busy(), tr(lang, K_AI_CHOOSE_REFERENCE))
                    .on_hover_text(tr(lang,
                        K_AI_REVERSE_FIT_TOWARD_ANY_FINISHED_VERSION_OF_THIS,
                    ))
                    .clicked()
                {
                    pick_ref = true;
                }
                if let Some(p) = self.workspace.fit().fit_ref.clone() {
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| p.display().to_string());
                    // The name and its ✕ share the row's second cell, laid
                    // right to left: the ✕ takes the cell's right end and the
                    // name is truncated into what is left (2026-10-14 — a
                    // truncating label took the whole remaining width and
                    // pushed the ✕ onto a line of its own). Truncated, never
                    // wrapped: a file name is one unbreakable word, and egui's
                    // inline layout in a wrapped row ran it past the panel
                    // edge; the full path is on hover.
                    ui.allocate_ui_with_layout(cell, Layout::right_to_left(Align::Center), |ui| {
                        // Bare glyph, not `tr` — the symbol IS the label in
                        // every language (the same rule the variant-strip and
                        // gallery ✕ buttons follow); only the tooltip is
                        // translated.
                        if glyph(ui, true, "✕")
                            .on_hover_text(tr(lang,
                                K_AI_FORGET_THIS_REFERENCE_AND_GO_BACK_TO_REVERSE_FIT,
                            ))
                            .clicked()
                        {
                            clear_ref = true;
                        }
                        ui.add(Label::new(Text::new(name).weak().small()).truncate())
                            .on_hover_text(p.display().to_string());
                    });
                }
            });
            // rfd OUTSIDE the closure above (it borrows `ui`) and after the row is
            // laid out: the dialog is modal and blocks this thread.
            if pick_ref && let Some(p) = util::photo_file_dialog() {
                self.workspace.fit_mut().fit_ref = Some(p);
            }
            if clear_ref {
                self.workspace.fit_mut().fit_ref = None;
            }
            // The fit's OWN Strength, directly above the verb that reads it (user
            // decision 2026-09-12: a control belongs to the section whose function
            // it serves). Through v1.3.1 the fit silently read the Analysis dial two
            // folds up — the last place anyone looking at this row would look. Same
            // helper as the Analysis dials: 0..100 track, double-click reset, hover
            // nudge, the explanation as the tooltip.
            slider_pct_hinted(
                ui,
                lang,
                tr(lang, K_AI_REVERSE_FIT_STRENGTH),
                &mut self.aip.fit_strength,
                1.0,
                autoshade::recipe::GradeStrength::DEFAULT,
                tr(lang,
                    K_AI_HOW_FAR_THE_FIT_MAY_MOVE_SLIDERS_ON_WEAK_EVIDENC,
                ),
            );
            ui.row_wrapped(|ui| {
                let cell = columns(ui, 2);
                let can = !self.workspace.busy() && can_fit;
                if primary_in(ui, cell, can, tr(lang, K_AI_REVERSE_FIT_RECIPE))
                    .on_hover_text(format!(
                        "{}{}",
                        fit_explainer(lang),
                        tr(lang, K_AI_USES_THE_REVERSE_FIT_STRENGTH_DIAL_ABOVE_AS_ITS),
                    ))
                    .clicked()
                {
                    self.apply_actions(vec![logic::Action::Ai(logic::ai::AiAction::ReverseFit)]);
                }
                if action_in(ui, cell, can, tr(lang, K_AI_EXTRACT_STYLE))
                    .on_hover_text(tr(lang,
                        K_AI_COMPARE_THE_ORIGINAL_GENERATED_IMAGES_AND_HAVE_T,
                    ))
                    .clicked()
                {
                    self.apply_actions(vec![logic::Action::Ai(logic::ai::AiAction::StylePrompt)]);
                }
            });
            // R20 opt-in LLM-as-a-judge. OUTSIDE the can_fit gate: the
            // toggle is a persisted PREFERENCE, not a fit-time verb —
            // gating it on can_fit locked a setting behind having a
            // generated variant active (review R20-N1). Only busy
            // disables it. R38: on a plain row of its own with 「deep」, never
            // in the wrapped verb row above — its two cells take the whole
            // line, and a checkbox scoped after them was laid past the line's
            // end (a scope never wraps), which widened the auto-fitting side
            // panel by 47 px every frame
            // (`the_generate_button_stays_one_line_and_the_panel_stays_put`
            // is the witness — that test exists because this panel's width
            // has run away once before).
            ui.row(|ui| {
                ui.enabled(!self.workspace.busy(), |ui| {
                    ui.checkbox(&mut self.workspace.fit_mut().fit_ai_judge, tr(lang, K_AI_AI_REVIEW))
                        .on_hover_text(tr(lang,
                            K_AI_AFTER_THE_FIT_SHOW_THE_TARGET_AND_THE_FITTED_REN,
                        ));
                });
                // R23-6 D: the review can now also ACT, when asked. Beside
                //「AI review」 and gated on it, because a deep fit IS that review
                // plus a loop — a checkbox that silently switched the other one
                // on would be two settings pretending to be one.
                ui.enabled(!self.workspace.busy() && self.workspace.fit().fit_ai_judge, |ui| {
                    ui.checkbox(&mut self.workspace.fit_mut().fit_deep, tr(lang, K_AI_RETRY_ONCE))
                        .on_hover_text(if self.workspace.fit().fit_ai_judge {
                            tr(lang,
                                K_AI_RETRY_ONCE_RUNS_THE_AI_REVIEW_BEFORE_SAVING_IF_T,
                            )
                        } else {
                            tr(lang, K_AI_TURN_ON_AI_REVIEW_FIRST_RETRY_ONCE_REPEATS_THAT)
                        });
                });
            });
            // The way back from the fit prompt's 「Don't ask again」 (v1.17.0),
            // beside the verb it asks about. A preference, so no gate.
            ui.checkbox(&mut self.workspace.fit_mut().ask_fit_after_generate, tr(lang, K_AI_ASK_AFTER_EACH_AI_GENERATION))
                .on_hover_text(tr(lang,
                    K_AI_WHEN_AI_GENERATE_LANDS_A_NEW_CARD_ASK_WHETHER_TO,
                ));
            // Migrated from Settings (#4): a switch that only ever changes what
            // the button above does. Same rule as 「AI review」 — a persisted
            // preference, so no can_fit gate (and, as in Settings, no busy gate:
            // `start_fit` reads the flag when it runs).
            let zoned_rows = |app: &mut Self, ui: &mut Ui| {
                ui.checkbox(&mut app.workspace.fit_mut().zoned_fit, tr(lang, K_AI_FIT_LOCAL_AREAS_TOO)).on_hover_text(tr(
                    lang,
                    K_AI_AFTER_THE_GLOBAL_FIT_TRIES_EXTRA_LOCAL_MASKS_WHE,
                ));
                ui.checkbox(&mut app.workspace.fit_mut().zoned_four_regions, tr(lang, K_AI_UP_TO_FOUR_SEMANTIC_REGIONS))
                    .on_hover_text(tr(lang, K_AI_OPT_IN_TO_SEMANTIC_REGIONS_BEYOND_THE_HISTORICAL));
            };
            zoned_rows(self, ui);
            });
    }
}

/// Bound on the analysis box's height before it scrolls: about eight body
/// lines — the verdict, a few lines of rationale and the first of the
/// working. The rationale's deterministic tail grows with every note a run
/// raises, so the box used to take whatever the text asked for and an
/// over-long analysis filled the screen (2026-10-13); the rest is one
/// 「View full text…」 away.
pub(crate) const ANALYSIS_BOX_MAX_H: f32 = 160.0;

/// The notes only a DEEP-THINKING analysis produces (`pipeline.rs`, R23-4's
/// three sentences + R23-1b's pixel-tool line): the model's working, as
/// opposed to the deterministic tail every analysis carries. ONE list, so the
/// box and the sentence agree on who takes which note.
pub(crate) fn is_working_note(key: autoshade::text::Key) -> bool {
    use autoshade::rationale::keys as k;
    [k::THINK_SCENE, k::THINK_LOOK, k::THINK_CRITIQUE, k::PIXEL_TOOLS].contains(&key)
}

/// A note's args as the `(key, value)` slice `trf` takes.
pub(crate) fn note_args(n: &autoshade::rationale::Note) -> Vec<(&str, &str)> {
    n.args.iter().map(|(k, v)| (*k, v.as_str())).collect()
}

/// Sub-area ⑤ — REVERSE-FIT (local; the AI review is paid): turn the
/// freshly generated look back into an
/// editable recipe — how the low-res experiment becomes a full-res, XMP-able
///「反推」variant. A PEER of the generation sub-area, not a row inside it:
/// its own switches (the optional AI review, and zoned fit — which used to
/// live in Settings, two panels from the button it changes) are settings for
/// this verb, and burying them one fold deeper than the verb was the
/// scattering this panel exists to end.
///
/// R30 gave it a header of its own (it was a `group_caption` under a
/// separator) and left it OPEN by default: the fit and the extract are
/// local and free, and the one paid switch inside — 「AI review」 — is
/// what the header's 「AI 打分付费」 half names.
/// What the reverse-fit does, in one sentence: the fold's verb says it on
/// hover and the fit prompt says it in its body.
pub(crate) fn fit_explainer(lang: Lang) -> &'static str {
    tr(lang,
        K_AI_STATISTICAL_FIT_REVERSE_THE_FRESHLY_GENERATED_LO,
    )
}

/// Sub-area ① continued — the deep-thinking WORKING as a block of its own.
///
/// User feedback 2026-09-11: listed inline in the rationale sentence, the
/// three R23-4 sentences and the R23-1b pixel-tool line ran the fold to a
/// wall of italic text. Here they are a captioned block under the
/// sentence, one row per note with the note's SUBJECT as a bold lead and
/// the model's own sentence selectable beside it — the arg text rides
/// verbatim, exactly as it did inside the sentence (the rationale
/// contract: model prose is never reworded on a surface). Its height is
/// bounded by the analysis box it sits in (2026-10-13), not by a scroll
/// of its own: a scroll inside that box would steal the wheel.
///
/// Drawn only when the caller's split actually took these notes OUT of
/// the sentence; on the fallback path they are still in it, and drawing
/// them too would print the working twice.
pub(crate) fn thinking_notes(lang: Lang, ui: &mut Ui, working: &[&autoshade::rationale::Note]) {
    use autoshade::rationale::keys;
    let lang = lang;
    ui.gap(SPACE_SM);
    ui.label(Text::new(tr(lang, K_AI_DEEP_THINKING_ITS_WORKING)).small().strong());
    for n in working {
        // The lead is OURS (one catalogue literal per key, so the i18n
        // audit sees each); the body is the note's single arg, the
        // model's sentence.
        let lead = match n.key {
            keys::THINK_SCENE => tr(lang, K_AI_WHAT_IT_SAW),
            keys::THINK_LOOK => tr(lang, K_AI_THE_LOOK_IT_AIMED_FOR),
            keys::THINK_CRITIQUE => tr(lang, K_AI_ITS_OWN_CRITIQUE_AGAINST_YOUR_STRENGTH_TARGET),
            keys::PIXEL_TOOLS => tr(lang, K_AI_PIXEL_TOOLS_IT_SUGGESTS_NOTHING_WAS_RUN),
            _ => continue,
        };
        let Some((_, body)) = n.args.first() else { continue };
        ui.row_wrapped(|ui| {
            ui.label(Text::new(lead).strong());
            ui.add(Label::new(body.as_str()).selectable(true));
        });
    }
}
