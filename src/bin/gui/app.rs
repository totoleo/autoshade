//! The application state, its defaults, and the eframe hooks.

use super::*;

/// The selected library recipe, developed on the open photo for the recipe
/// window's preview pane. `key` is everything the image depends on — the
/// entry's name and file stamp, the photo, and the base pixels' allocation —
/// so a changed selection, a re-saved entry or a different photo rebuilds it.
pub(crate) struct RecipePreview {
    pub(crate) key: String,
    pub(crate) texture: TextureHandle,
}

pub(crate) struct AutoShadeApp {
    /// The open photo's edit: pixels, document, session, analysis, canvas
    /// plates and the develop scheduler (see [`Editor`]).
    editor: Editor,
    /// The client-only view/session state (see [`WorkspaceView`]).
    workspace: WorkspaceView,
    /// The worker channel and the event-loop context (see [`Services`]).
    services: Services,
    // TYPED, not a baked string: the decision word + reasons are rendered
    // (and translated) at draw time, so a language switch re-renders them.
    /// Scripts already disclosed as undrawable this session (L12#3) — the
    /// tofu warning fires once per script, not once per folder or frame.
    /// The rationale's deterministic tail as typed notes (L12#2B). They
    /// render, in order, `rationale`'s SUFFIX — the panel strips it and
    /// draws the notes localized; on any mismatch (or when empty — a
    /// develop restored from disk carries none) the raw English rationale
    /// shows instead. Cleared wherever `rationale` is reloaded from disk.
    /// What guides AI Analyze: the reference library or the Direction text —
    /// either/or (user ruling 2026-10-14). The controls of the one not chosen
    /// are greyed out. Persisted.
    /// The persisted AI preferences; see [`AiPrefs`].
    aip: AiPrefs,
    lang: Lang,             // UI language: English skeleton / Chinese overlay (i18n.rs)
    /// Layout/behaviour probes for the window tests; see [`TestSeams`].
    #[cfg(test)]
    seams: TestSeams,
}

impl AutoShadeApp {
    /// The root's state stays its own: a caller borrows ONE piece for the
    /// length of one call. Reads take the shared door; writes the `_mut` one.
    #[cfg(test)]
    pub(crate) fn editor(&self) -> &Editor {
        &self.editor
    }

    #[cfg(test)]
    pub(crate) fn editor_mut(&mut self) -> &mut Editor {
        &mut self.editor
    }

    #[cfg(test)]
    pub(crate) fn workspace(&self) -> &WorkspaceView {
        &self.workspace
    }

    #[cfg(test)]
    pub(crate) fn workspace_mut(&mut self) -> &mut WorkspaceView {
        &mut self.workspace
    }

    #[cfg(test)]
    pub(crate) fn services(&self) -> &Services {
        &self.services
    }

    #[cfg(test)]
    pub(crate) fn aip(&self) -> &AiPrefs {
        &self.aip
    }

    #[cfg(test)]
    pub(crate) fn aip_mut(&mut self) -> &mut AiPrefs {
        &mut self.aip
    }

    #[cfg(test)]
    pub(crate) fn lang(&self) -> Lang {
        self.lang
    }

    #[cfg(test)]
    pub(crate) fn lang_mut(&mut self) -> &mut Lang {
        &mut self.lang
    }

    /// The editor and the view state TOGETHER, for the invariants that take
    /// both (a history step, a rotate, a skin switch): two doors would borrow
    /// the root twice, one pair borrows each field once.
    #[cfg(test)]
    pub(crate) fn editor_workspace_mut(&mut self) -> (&mut Editor, &mut WorkspaceView, Lang) {
        (&mut self.editor, &mut self.workspace, self.lang)
    }

    #[cfg(test)]
    pub(crate) fn seams(&mut self) -> &mut TestSeams {
        &mut self.seams
    }

    /// Test builders: a fixture starts from `Default` and overrides ONE piece,
    /// which is what the struct literals used to spell out. The fields stay the
    /// root's own; a test may inject a stand-in through a named door.
    #[cfg(test)]
    pub(crate) fn but_editor(mut self, editor: Editor) -> Self {
        self.editor = editor;
        self
    }

    #[cfg(test)]
    pub(crate) fn but_workspace(mut self, workspace: WorkspaceView) -> Self {
        self.workspace = workspace;
        self
    }

    #[cfg(test)]
    pub(crate) fn but_aip(mut self, aip: AiPrefs) -> Self {
        self.aip = aip;
        self
    }

    #[cfg(test)]
    pub(crate) fn but_lang(mut self, lang: Lang) -> Self {
        self.lang = lang;
        self
    }

    #[cfg(test)]
    pub(crate) fn but_seams(mut self, seams: TestSeams) -> Self {
        self.seams = seams;
        self
    }

    /// Restore persisted prefs (last folder, view mode, export options) and
    /// re-open the library the user was browsing. Window geometry itself is
    /// restored by eframe's own persistence layer.
    pub(crate) fn new(ctx: &Ctx, prefs: Option<Prefs>) -> Self {
        // The REAL event-loop context replaces default()'s headless one:
        // workers wake the loop through this clone (see `spawn_worker`).
        let mut app = Self {
            services: Services { view_ctx: ctx.clone(), ..Services::default() },
            ..Self::default()
        };
        if let Some(prefs) = prefs {
            app.restore_prefs(prefs);
        }
        // The launch check of the interpreter the AI sidecars would run: a fact
        // about this machine, measured once before the first frame. It spawns
        // the interpreter only when the managed environment is not the answer,
        // so a prepared install costs one file check. After the prefs restore
        // (which decides the language) and outside it, so headless tests that
        // restore prefs run no interpreter.
        if let Some(note) = app.commands().refresh_python_check() {
            app.workspace.chrome_mut().startup_note = Some(note);
        }
        if !app.workspace.busy() {
            app.workspace.show_startup_note();
        }
        // The quit guard's two process-wide hooks (see `quit.rs`): the repaint
        // handle a vetoed quit wakes the UI with, and — on macOS — the
        // delegate method that does the vetoing. Both need a live event loop,
        // and this creation closure is the first moment there is one.
        crate::quit::set_ctx(ctx);
        #[cfg(target_os = "macos")]
        if let Err(why) = crate::macos::install_quit_guard() {
            // Not fatal, and not silent: without that method ⌘Q behaves the
            // way it did before this batch, and the user is owed the fact.
            eprintln!(
                "warning: ⌘Q and Dock Quit will NOT ask about unsaved work — {why}. \
                 Close the window with its red button instead, which does ask."
            );
        }
        app
    }

    /// Restore the persisted settings through the same path at startup and
    /// in headless tests; no access to the user's actual prefs is needed.
    pub(crate) fn restore_prefs(&mut self, prefs: Prefs) {
        let app = self;
        app.aip.guide = prefs.guide;
        app.aip.style_strength = crate::logic::prefs::clamp01(prefs.style_strength);
        app.aip.grade_strength = crate::logic::prefs::clamp01(prefs.grade_strength);
        app.aip.fit_strength = crate::logic::prefs::clamp01(prefs.fit_strength);
        app.aip.send_style_ref_image = prefs.send_style_ref_image;
        app.aip.deep_think = prefs.deep_think;
        app.aip.style_embed = prefs.style_embed;
        app.aip.style_describe = prefs.style_describe;
        // Only a folder that still EXISTS is prefilled: a picker opened at
        // a deleted path lands wherever the OS decides (the same rule the
        // gallery restore above follows).
        app.aip.style_src_dir = prefs.style_src_dir.clone().filter(|d| d.is_dir());
        app.aip.style_raw_dir = prefs.style_raw_dir.clone().filter(|d| d.is_dir());
        app.aip.direction_adherence = autoshade::recipe::DirectionAdherence::new(prefs.direction_adherence).get();
        app.workspace.exp_mut().format = ExportFormat::from_pref(prefs.exp_format, prefs.save_jpeg);
        app.workspace.exp_mut().dest = ExportDest::from_pref(prefs.exp_dest);
        app.workspace.exp_mut().last_dir = prefs.last_export_dir.clone();
        app.workspace.exp_mut().save_denoise = prefs.save_denoise;
        app.workspace.exp_mut().save_denoise_strength = crate::logic::prefs::clamp01(prefs.save_denoise_strength);
        app.workspace.exp_mut().denoise_strength = crate::logic::prefs::clamp01(prefs.denoise_strength);
        app.workspace.exp_mut().baked_denoise_strength = crate::logic::prefs::clamp01(prefs.baked_denoise_strength);
        app.workspace.exp_mut().baked_save_denoise_strength = crate::logic::prefs::clamp01(prefs.baked_save_denoise_strength);
        app.workspace.fit_mut().zoned_fit = prefs.zoned_fit;
        app.workspace.fit_mut().zoned_four_regions = prefs.zoned_four_regions;
        app.workspace.fit_mut().fit_ai_judge = prefs.fit_ai_judge;
        app.workspace.fit_mut().fit_deep = prefs.fit_deep;
        app.workspace.fit_mut().reimagine_retry = prefs.reimagine_retry;
        app.workspace.fit_mut().ask_fit_after_generate = prefs.ask_fit_after_generate;
        app.workspace.genai_mut().adjust_quality = crate::logic::prefs::choice(prefs.adjust_quality);
        app.workspace.view_mut().mode = prefs.view_mode;
        app.workspace.exp_mut().long_edge = prefs.exp_long_edge;
        app.workspace.exp_mut().sharpen = crate::logic::prefs::clamp_sharpen(prefs.exp_sharpen);
        app.workspace.exp_mut().quality = crate::logic::prefs::clamp_quality(prefs.exp_quality);
        app.workspace.chrome_mut().show_clipping = prefs.show_clipping;
        // Restore the UI language (an older save without this key decoded to
        // `Lang::En` via `#[serde(default)]`, so this is always valid).
        app.lang = prefs.lang;
        // Restore the theme and re-apply it — main() installed the Dark
        // default before prefs were readable (same shape as the greeting).
        app.workspace.ui_mut().theme = prefs.theme;
        app.workspace.ui_mut().accent = prefs.accent;
        install_look(&app.services.view_ctx, app.workspace.ui().theme, app.workspace.ui().accent);
        // The greeting was set by default() BEFORE the language was known —
        // re-issue it, or a Chinese install opens with one English line.
        app.workspace.set_status(tr(app.lang, K_ACTIONS_OPEN_A_PHOTO_OR_OPEN_A_FOLDER_TO_BROWSE_YOUR_LIB).into());
        // Each saved file advances to the current dial law; a current/future
        // file keeps the choices made under its own law. The wording of the
        // reset note uses the values the file actually carried.
        for era in prefs.prefs_era..PREFS_ERA {
            let migrated = crate::logic::prefs::migrate_era(era, prefs.denoise_strength, prefs.save_denoise_strength);
            app.workspace.exp_mut().denoise_strength = migrated.raw_detail;
            app.workspace.exp_mut().save_denoise_strength = migrated.raw_export;
            if let Some(v) = migrated.baked_detail {
                app.workspace.exp_mut().baked_denoise_strength = v;
            }
            if let Some(v) = migrated.baked_export {
                app.workspace.exp_mut().baked_save_denoise_strength = v;
            }
            if let Some((a, b)) = migrated.reset_note {
                let default = autoshade::denoise::DEFAULT_STRENGTH_RAW;
                app.workspace.chrome_mut().startup_note = Some(trf(
                    app.lang,
                    K_ACTIONS_AI_DENOISE_STRENGTH_FOR_RAW_FILES_WAS_RESET_TO_W,
                    &[
                        ("new", &format!("{:.0}", default * 100.0)),
                        ("a", &format!("{:.0}", a * 100.0)),
                        ("b", &format!("{:.0}", b * 100.0)),
                    ],
                ));
            }
        }
        // Only known color spaces and working-preview steps: a corrupt pref
        // must not produce an unrenderable space or a 1-px / 100-MP preview.
        app.workspace.exp_mut().space = crate::logic::prefs::known_space(prefs.exp_space);
        app.workspace.genai_mut().preview_edge = crate::logic::prefs::known_preview_edge(prefs.preview_edge);
        if let Some(dir) = prefs.gallery_dir.filter(|d| d.is_dir()) {
            app.commands().open_folder(dir);
        }
        // A restored folder's asynchronous status must not erase the note
        // before the user sees it. Its completion uses the same status line.
        if !app.workspace.busy() {
            app.workspace.show_startup_note();
        }
    }













    /// Match a live card to a persisted card. Stable IDs are authoritative
    /// when both sides carry one; an id-less legacy side falls back to kind +
    /// strip position, mirroring [`version_is_from`]'s ID-first rule.
    pub(crate) fn persisted_identity_matches(
        live: &Variant,
        live_position: usize,
        saved_kind: VariantKind,
        saved_position: usize,
        saved_id: Option<&str>,
    ) -> bool {
        crate::logic::strip::identity_matches(
            &live.id,
            live.kind,
            live_position,
            saved_kind,
            saved_position,
            saved_id,
        )
    }

    pub(crate) fn poll_workers(&mut self, ctx: &Ctx) {
        // UI-thread language for the status/toast strings built HERE, at
        // landing. Worker results carry typed FACTS (FitOutcome, RetouchNote,
        // PasteOutcome, …) that the landing arms render in the language live
        // NOW (L12#4 — a sentence translated inside the spawn closure carried
        // the language captured at spawn, minutes stale by the time it
        // landed). The Strings that still ride the channel are payloads, not
        // pre-translated sentences: a generative worker's heartbeat line, a
        // mask's stable English label and the sidecar's backend name, an
        // extracted style prompt, a model id, an error's own text.
        let lang = self.lang;
        // Drain a bounded batch each frame so a burst of thumbnails doesn't take
        // one-per-frame to land (try_recv borrow is released before we mutate).
        for _ in 0..64 {
            let Some(msg) = self.services.rx.as_ref().and_then(|rx| rx.try_recv().ok()) else { break };
            match msg {
                Msg::Opened(boxed) => {
                    self.commands().on_opened(ctx, lang, boxed);
                    // A Lightroom AI mask arrives without an alpha: cut it
                    // on this computer now, or the canvas skips it.
                    self.commands().start_ai_mask_resolve(lang);
                }
                Msg::BaseRedeveloped { path, part, img } => {
                    self.commands().on_base_redeveloped(ctx, path, *part, *img)
                }
                Msg::AiMasksResolved(boxed) => self.commands().on_ai_masks_resolved(lang, *boxed),
                Msg::Developed(boxed) => self.commands().finish_redevelop(ctx, *boxed),
                Msg::Analyzed(epoch, mut boxed) => {
                    // The look protocol's claim: only the pending look's own,
                    // current epoch may touch this result. A looked-for verb
                    // also puts back what its fresh look replaced, BEFORE the
                    // shared landing writes.
                    let gen_epoch = self.workspace.tools().gen_epoch;
                    let claim = logic::look::claim(&mut self.workspace.tools_mut().look, epoch, gen_epoch, &boxed);
                    if let Some(claim) = &claim
                        && let Ok((recipe, _, _)) = &mut *boxed
                    {
                        logic::look::restore(claim, recipe);
                    }
                    let landed = boxed.is_ok();
                    self.commands().on_analyzed(lang, epoch, boxed);
                    // The claimed look's own line, memory and view facts.
                    if let Some(claim) = claim {
                        let effects = logic::look::landed(&claim, &self.editor.doc().recipe, landed);
                        self.commands().run_effects(effects);
                    }
                }
                Msg::OneClickBuildProgress { done, total } => {
                    self.workspace.normal_mut().build_progress = Some((done, total));
                }
                Msg::OneClickBuilt(res) => self.commands().on_one_click_built(lang, res),
                Msg::PythonEnvLine(line) => self.workspace.ui_mut().python_env_line = Some(line),
                Msg::PythonEnvReady(res) => self.commands().on_python_env_ready(lang, res),
                Msg::PortraitFound(found) => self.editor.on_portrait_found(&mut self.workspace, lang, *found),
                Msg::Exported(Ok(outcome)) => {
                    // Normal's Save group words its own landing (`normal.rs`).
                    let plain = self.workspace.normal_delivery_words(lang, &outcome);
                    self.workspace.exp_mut().batch_progress = None; // the bar belongs to ONE batch run
                    match outcome {
                        ExportOutcome::Single { out, relooked, warn } => {
                            // ABSOLUTE (R22-7): the deliverable exists NOW, and
                            // a folder relative to the launch directory was the
                            // single most-reported "where did my file go".
                            let p = abs_display(&out);
                            if relooked
                                && let Some(photo) = self.editor.photo().src_path.clone()
                                && autoshade::pipeline::repair_pre_era_base_curve(
                                    &photo,
                                    &mut self.editor.doc_mut().recipe,
                                )
                                .is_some()
                            {
                                // The export repaired its own COPY: the canvas
                                // is repaired too, or export and preview stay
                                // divergent for the rest of the session.
                                self.editor.session_mut().dirty = true;
                            }
                            let words =
                                logic::export_landing::single_words(&p, relooked, warn.as_deref());
                            let mut line = crate::i18n::text(lang, &words.line);
                            if let Some(relook) = words.relook {
                                line = format!("{line} — {}", crate::i18n::text(lang, &relook));
                            }
                            match words.warn {
                                Some(warn) => {
                                    let t = format!(
                                        "{line} · {}",
                                        crate::i18n::text(lang, &warn)
                                    );
                                    self.workspace.set_status(t.clone());
                                    self.workspace.toast(ToastKind::Error, t);
                                }
                                None => self.workspace.done(line),
                            }
                        }
                        ExportOutcome::Batch { ok, errs, renamed, relooked, warns, dest } => {
                            // Where the batch actually DELIVERED becomes the
                            // remembered destination — here, not at the dialog
                            // (R22 M1): a folder inside the photo library is
                            // refused per photo, so a batch that picked one
                            // lands nothing and must not be remembered. `ok > 0`
                            // is the proof that at least one file is there.
                            if ok > 0 {
                                self.workspace.exp_mut().last_dir = Some(dest.clone());
                            }
                            let words = logic::export_landing::batch_words(
                                ok,
                                &errs,
                                &renamed,
                                relooked,
                                &warns,
                                &abs_display(&dest),
                            );
                            let mut line = crate::i18n::text(lang, &words.base);
                            for part in [words.renamed, words.relooked, words.develops]
                                .into_iter()
                                .flatten()
                            {
                                line.push_str(&crate::i18n::text(lang, &part));
                            }
                            if errs.is_empty() {
                                self.workspace.done(line);
                            } else {
                                // A partial failure keeps the error channel,
                                // exactly as the worker-side bail! did.
                                self.workspace.fail(tr(lang, K_GUI_EXPORT_FAILED), anyhow::anyhow!("{line}"));
                            }
                        }
                    }
                    if let Some(plain) = plain {
                        self.workspace.reword_landing(plain);
                    }
                }
                Msg::Exported(Err(e)) => {
                    self.workspace.exp_mut().batch_progress = None;
                    self.workspace.normal_mut().delivering = false;
                    self.workspace.fail(tr(lang, K_GUI_EXPORT_FAILED), e);
                }
                Msg::BatchProgress { done, total } => {
                    self.workspace.exp_mut().batch_progress = Some((done, total));
                    // No destination here: the tick fires once per photo and the
                    // root was already named when the batch started (and is
                    // named again at the landing). Repeating it 500 times only
                    // pushed the counts out of a truncated status line — and
                    // the old hardcoded "./out" became a lie the moment the
                    // destination became a setting (R22-7).
                    *self.workspace.status_mut() = trf(
                        lang,
                        K_GUI_BATCH_RENDERING,
                        &[("done", &done.to_string()), ("total", &total.to_string())],
                    );
                }
                Msg::StyleInfo(info) => {
                    self.workspace.ai_mut().style_info_loading = false;
                    self.workspace.ai_mut().style_info = Some(*info);
                }
                Msg::StyleBuildProgress { stage, done, total } => {
                    self.workspace.ai_mut().style_build_progress = Some((stage, done, total));
                    // The panel's own line renders these counts; the status bar
                    // gets them too, because a minutes-long job must be visible
                    // from wherever the user is looking.
                    *self.workspace.status_mut() = trf(
                        lang,
                        K_GUI_BUILDING_THE_STYLE_LIBRARY_PHOTOS,
                        &[
                            ("stage", tr(lang, stage.label())),
                            ("done", &done.to_string()),
                            ("total", &total.to_string()),
                        ],
                    );
                }
                Msg::StyleBuilt(outcome) => self.commands().on_style_built(lang, *outcome),
                Msg::Segmented(res) => self.commands().on_segmented(lang, res),
                Msg::DepthEstimated(res) => {
                    self.commands().on_depth_estimated(res);
                    self.editor.after_portrait_depth(&mut self.workspace);
                }
                Msg::MaskRefined(res) => self.commands().on_mask_refined(lang, res),
                Msg::Folder(boxed) => {
                    self.commands().on_folder(lang, *boxed);
                    self.workspace.show_startup_note();
                }
                Msg::Thumb { generation, idx, img } => self.workspace.on_thumb(ctx, generation, idx, *img),
                Msg::MasterLoaded { photo, origin, edge, stamp, img } => {
                    self.commands().on_master_loaded(ctx, lang, photo, origin, edge, stamp, *img)
                }
                Msg::Progress(epoch, m) => {
                    // Liveness lines from a running generative worker. Epoch-
                    // gated: after Cancel + an immediate re-run, the abandoned
                    // worker's heartbeats must not overwrite the new task's
                    // status line.
                    if self.workspace.busy() && self.workspace.tools().gen_cancel.is_some() && epoch == self.workspace.tools().gen_epoch {
                        self.workspace.set_status(m);
                    }
                }
                Msg::Retouched(epoch, boxed) => {
                    // Normal's noise switch lands through the same door —
                    // with the look's undo history kept across its card.
                    let history = self.commands().normal_denoise_history();
                    self.commands().on_retouched(ctx, lang, epoch, *boxed);
                    self.commands().after_normal_denoise(lang, history);
                }
                Msg::Fitted(boxed) => self.commands().on_fitted(ctx, lang, boxed),
                Msg::Styled(boxed) => match *boxed {
                    Ok((prompt, note)) => {
                        // Into the Reimagine prompt: ready to restyle OTHER photos.
                        self.workspace.genai_mut().reimagine_prompt = prompt;
                        let s = match &note {
                            StyleNote::SavedCopy => tr(
                                lang,
                                K_GUI_STYLE_PROMPT_EXTRACTED_FILLED_INTO_THE_GENERATE,
                            )
                            .to_string(),
                            StyleNote::SaveFailed(err) => trf(
                                lang,
                                K_GUI_STYLE_PROMPT_EXTRACTED_FILLED_INTO_THE_GENERATE__2,
                                &[("err", err)],
                            ),
                            StyleNote::NotSaved => tr(
                                lang,
                                K_GUI_STYLE_PROMPT_EXTRACTED_FILLED_INTO_THE_GENERATE__3,
                            )
                            .to_string(),
                        };
                        self.workspace.done(s);
                    }
                    Err(e) => {
                        self.workspace.fail(tr(lang, K_GUI_STYLE_EXTRACTION_FAILED), e);
                    }
                },
                Msg::Pasted(res) => {
                    // Normal's 「apply to every photo」 renders once the paste
                    // has landed whole (`normal.rs`).
                    let then_render = self.workspace.normal_mut()
                        .apply_all
                        .take()
                        .map(|dest| (dest, res.as_ref().is_ok_and(|o| o.errs.is_empty())));
                    self.commands().on_pasted(lang, res);
                    if let Some((dest, pasted)) = then_render {
                        self.commands().after_apply_all_paste(lang, dest, pasted);
                    }
                }
                Msg::LegacyImported(res) => {
                    self.workspace.gal_mut().edited_badge.clear(); // imported sidecars light ● badges
                    match res {
                        Ok((n, dir)) => self.workspace.done(trf(
                            lang,
                            K_GUI_IMPORTED_SAVED_DEVELOPS_FOR_PHOTO_S_FROM,
                            &[("n", &n.to_string()), ("path", &dir.display().to_string())],
                        )),
                        Err(e) => self.workspace.fail(tr(lang, K_GUI_IMPORT_FAILED), e),
                    }
                }
                Msg::Models(role, generation, res) => self.workspace.on_models(lang, role, generation, res),
            }
        }
    }


    

    /// The verb projection: the two aggregates, the worker channel, the AI
    /// preferences and the language — borrowed for the length of one command.
    /// The behaviour modules implement their verbs on it, so the root stays
    /// wiring.
    pub(crate) fn commands(&mut self) -> Commands<'_> {
        Commands {
            editor: &mut self.editor,
            workspace: &mut self.workspace,
            services: &self.services,
            aip: &mut self.aip,
            lang: self.lang,
            #[cfg(test)]
            seams: &mut self.seams,
        }
    }

    /// The projection a panel draws through: the edit, the view state, the
    /// language and the test seams — never the app itself.
    pub(crate) fn panel_view<'a>(&'a mut self, actions: &'a mut Vec<logic::Action>) -> PanelView<'a, 'a> {
        PanelView {
            editor: &mut self.editor,
            workspace: &mut self.workspace,
            lang: self.lang,
            aip: &mut self.aip,
            actions,
            #[cfg(test)]
            seams: &mut self.seams,
        }
    }

    /// Draw one projected panel and run the intents it reported.
    #[cfg(test)]
    pub(crate) fn draw_panel(&mut self, draw: impl FnOnce(&mut PanelView<'_, '_>)) {
        let mut actions = Vec::new();
        draw(&mut self.panel_view(&mut actions));
        self.commands().apply_actions(actions);
    }

    /// One update() phase — body extracted verbatim from the eframe
    /// update loop (round-12 decomposition).
    // pub(crate): the zoom-keys test drives a real headless frame through it.
    pub(crate) fn upd_shortcuts(&mut self, ctx: &Ctx) {
        // Tab's egui focus traversal fired last frame despite the consume (see
        // `defocus_next`): drop that focus now so the panel toggle doesn't
        // leave a surprise-focused widget eating every later shortcut.
        if std::mem::take(&mut self.workspace.ui_mut().defocus_next)
            && let Some(id) = ctx.memory(|m| m.focused())
        {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        // Global shortcuts, in THREE tiers (all skipped while a modal layer
        // is up — the quit confirm, where no key must mutate the very state
        // the user is deciding whether to save, or the fit prompt, whose
        // answer is about the card under it; the Settings / ⌨ windows are
        // keyboard-modal too, D13):
        //  * Esc while a transient window is up dismisses it.
        //  * Ctrl+O/E/S have no text-editing meaning, so they work even while
        //    a widget holds focus — focusing a prompt used to silently kill
        //    Save/Open/Export until the user clicked elsewhere.
        //  * Everything else (plain letters, arrows, Ctrl+Z/Y) stays gated on
        //    "no widget focused": typing must type, and a text field's own
        //    undo owns Ctrl+Z there.
        // Ctrl+Z/Y = undo/redo, Ctrl+O = open, Ctrl(+Shift)+E = export,
        // Ctrl+S = save XMP, ←/→ = walk the gallery (crop armed: nudge the
        // box), R = crop, Enter = commit crop, W/Q/K/M = LR tool keys,
        // \ / Y = compare, [ ] = brush size, Tab = hide the side panels —
        // the keyboard grammar of every desktop photo editor.
        let transient_open = self.workspace.ui().show_settings || self.workspace.ui().show_shortcuts;
        if !self.workspace.modal_up() && transient_open {
            let sheet_open = self.workspace.ui().show_shortcuts;
            let (esc, sheet_key) = ctx.input_mut(|i| {
                (
                    Chord::ESCAPE.consume(i),
                    // F1 / ? stay live while the sheet is open — they are its
                    // advertised toggle, and the focused-none tier below is
                    // skipped entirely while a transient is up (L19).
                    sheet_open
                        && (Chord::F1.consume(i)
                            || Chord::QUESTION.consume(i)
                            || Chord::SHIFT_QUESTION.consume(i)),
                )
            });
            if esc || sheet_key {
                // Cheat-sheet first — it is the topmost transient.
                if self.workspace.ui().show_shortcuts {
                    self.workspace.ui_mut().show_shortcuts = false;
                } else {
                    self.workspace.ui_mut().show_settings = false;
                }
            }
        }
        if !self.workspace.modal_up() && !transient_open {
            let (mut do_open, mut do_export, mut do_xmp) = (false, false, false);
            ctx.input_mut(|i| {
                if Chord::CTRL_O.consume(i) { do_open = true; }
                // LR's export key is Ctrl+Shift+E — consumed FIRST (the
                // exact-match consume would leave it dead behind plain
                // Ctrl+E, which stays as the historical alias, D13).
                if Chord::CTRL_SHIFT_E.consume(i) { do_export = true; }
                if Chord::CTRL_E.consume(i) { do_export = true; }
                if Chord::CTRL_S.consume(i) { do_xmp = true; }
            });
            if do_open && !self.workspace.busy() {
                self.workspace.gal_mut().selected = None;
                self.commands().apply_actions(vec![logic::Action::Gallery(
                    logic::gallery::GalleryAction::OpenPhotoFile,
                )]);
            }
            if do_export && self.editor.photo().src_path.is_some() && !self.workspace.busy() {
                self.commands().start_export();
            }
            if do_xmp && self.editor.photo().src_path.is_some() && !self.workspace.busy() {
                // The pending-rename flush lives at save_xmp's head now (it
                // fires while a TextEdit holds focus either way) — the Save
                // develop BUTTON takes the same entry point (U10).
                self.commands().save_xmp();
            }
        }
        if !self.workspace.modal_up() && !transient_open && ctx.memory(|m| m.focused()).is_none() {
            let (mut do_undo, mut do_redo) = (false, false);
            let (mut do_escape, mut do_overlay, mut do_clip) = (false, false, false);
            let (mut do_cheatsheet, mut do_crop, mut do_panels) = (false, false, false);
            let (mut do_crop_commit, mut do_crop_grid) = (false, false);
            let (mut do_wb, mut do_heal, mut do_brush) = (false, false, false);
            let (mut do_linear, mut do_radial) = (false, false);
            let (mut do_before, mut do_compare) = (false, false);
            let (mut do_fit, mut do_one) = (false, false);
            let (mut do_copy, mut do_paste) = (false, false);
            let mut zoom_key: f32 = 0.0;
            let mut crop_nudge = (0.0f32, 0.0f32);
            let mut nav: i32 = 0;
            let mut brush_delta: f32 = 0.0;
            // [ / ] belong to the active brush tool; when none is armed the
            // keys stay unconsumed (free for egui / future bindings).
            let brush_tool = self.workspace.masks().paint_mode || self.workspace.tools().clone_mode;
            let crop_tool = self.workspace.crop().mode;
            // ↑/↓ walk the library ONLY while the pointer is off the controls
            // panel: over it, the same keys nudge a hovered slider — and this
            // handler runs FIRST each frame, so consuming here would eat the
            // nudge before the slider ever saw the key. Last frame's rect
            // (panels draw after the shortcuts) — one frame stale, harmless.
            let over_controls = ctx
                .input(|i| i.pointer.latest_pos())
                .zip(self.workspace.view().controls_rect)
                .is_some_and(|(p, r)| r.contains(p));
            ctx.input_mut(|i| {
                if Chord::CTRL_SHIFT_Z.consume(i) { do_redo = true; }
                if Chord::CTRL_Y.consume(i) { do_redo = true; }
                if Chord::CTRL_Z.consume(i) { do_undo = true; }
                if crop_tool {
                    // Crop owns the arrows while armed (keyboard-only
                    // geometry, D13); gallery walking resumes on exit.
                    let s = if i.modifiers.shift { 10.0 } else { 1.0 };
                    for (chord, dx, dy) in [
                        (Chord::ARROW_RIGHT, 1.0, 0.0),
                        (Chord::ARROW_LEFT, -1.0, 0.0),
                        (Chord::ARROW_DOWN, 0.0, 1.0),
                        (Chord::ARROW_UP, 0.0, -1.0),
                    ] {
                        if chord.consume(i) || chord.with_shift().consume(i) {
                            crop_nudge.0 += dx * s;
                            crop_nudge.1 += dy * s;
                        }
                    }
                    if Chord::ENTER.consume(i) { do_crop_commit = true; }
                }
                if Chord::ARROW_RIGHT.consume(i) { nav = 1; }
                if Chord::ARROW_LEFT.consume(i) { nav = -1; }
                // ↑/↓: the vertical twins (a vertical list wants vertical
                // keys) — gated on `over_controls` above.
                if !over_controls {
                    if Chord::ARROW_DOWN.consume(i) { nav = 1; }
                    if Chord::ARROW_UP.consume(i) { nav = -1; }
                }
                // Zoom keys — the canvas Fit / 1:1 buttons' twins. `=` is
                // unshifted `+` on ANSI layouts; Shift++ covers the rest.
                if Chord::PLUS.consume(i)
                    || Chord::SHIFT_PLUS.consume(i)
                    || Chord::EQUALS.consume(i)
                {
                    zoom_key = 1.0;
                }
                if Chord::MINUS.consume(i) { zoom_key = -1.0; }
                if Chord::NUM0.consume(i) { do_fit = true; }
                if Chord::NUM1.consume(i) { do_one = true; }
                // Ctrl+Shift+C/V (LR's Copy/Paste Develop Settings):
                // keyboard twins of ⎘ Copy recipe / ⇩ Paste to selected.
                // EVENT-level, not consume_key: egui-winit swallows every
                // Ctrl(+Shift)+C into Event::Copy and emits NO Key event
                // (egui-winit lib.rs is_copy_command — the Codex 阶段5
                // review caught the Key-based version as unreachable on
                // real input). Shift is the discriminator: a plain Ctrl+C
                // keeps egui's own selected-text copy, the Shift chord is
                // ours and the event is removed so nothing double-fires.
                if i.modifiers.command && i.modifiers.shift {
                    let n = i.events.len();
                    i.events.retain(|e| !matches!(e, Event::Copy));
                    do_copy |= i.events.len() < n;
                    let n = i.events.len();
                    i.events.retain(|e| !matches!(e, Event::Paste(_)));
                    do_paste |= i.events.len() < n;
                }
                if Chord::ESCAPE.consume(i) { do_escape = true; }
                if Chord::O.consume(i) {
                    if crop_tool { do_crop_grid = true; } else { do_overlay = true; }
                }
                if Chord::J.consume(i) { do_clip = true; }
                if Chord::R.consume(i) { do_crop = true; }
                // LR tool keys (D13): W / Q / K and M / Shift+M (the Shift
                // variant consumed first — exact match leaves it dead after).
                if Chord::W.consume(i) { do_wb = true; }
                if Chord::Q.consume(i) { do_heal = true; }
                if Chord::K.consume(i) { do_brush = true; }
                if Chord::SHIFT_M.consume(i) { do_radial = true; }
                if Chord::M.consume(i) { do_linear = true; }
                if Chord::BACKSLASH.consume(i) { do_before = true; }
                if Chord::Y.consume(i) { do_compare = true; }
                if Chord::TAB.consume(i) { do_panels = true; }
                if brush_tool {
                    if Chord::OPEN_BRACKET.consume(i) { brush_delta = -4.0; }
                    if Chord::CLOSE_BRACKET.consume(i) { brush_delta = 4.0; }
                }
                // F1 / ? — the cheat-sheet (Shift+/ produces ? on most layouts).
                if Chord::F1.consume(i)
                    || Chord::QUESTION.consume(i)
                    || Chord::SHIFT_QUESTION.consume(i)
                {
                    do_cheatsheet = true;
                }
            });
            // Mirror the ↶/↷ BUTTONS exactly — `add_enabled(ready &&
            // !stack.is_empty())` with `ready = src_path.is_some() && !busy`.
            // Un-gated, Ctrl+Z during a retouch swapped the base the landing
            // result is applied over and, since every pixel undo now writes a
            // status, wiped the only liveness the bar was showing for a
            // worker that sends no progress. But the BUSY half alone made the
            // refusal lie: Ctrl+Z during a folder scan with no photo open
            // drew "busy — …unlock when the current task finishes" for an
            // unlock that would never come (no photo, no history). Only a
            // press that WOULD have acted earns a refusal; the rest stay the
            // silent no-ops the disabled buttons are.
            let armed_undo = do_undo && self.editor.photo().src_path.is_some() && !self.editor.session().hist.undo_stack.is_empty();
            let armed_redo = do_redo && self.editor.photo().src_path.is_some() && !self.editor.session().hist.redo_stack.is_empty();
            if self.workspace.busy() {
                if armed_undo || armed_redo {
                    let t = tr(
                        self.lang,
                        K_APP_BUSY_UNDO_AND_REDO_UNLOCK_WHEN_THE_CURRENT_TASK,
                    );
                    self.workspace.toast(ToastKind::Error, t);
                }
            } else {
                if armed_undo { self.editor.undo(&mut self.workspace, self.lang, ctx); }
                if armed_redo { self.editor.redo(&mut self.workspace, self.lang, ctx); }
            }
            // R mirrors the crop button exactly (incl. the one-tool-at-a-time
            // disarms); no photo → nothing to crop.
            if do_crop && self.editor.photo().src_path.is_some() {
                let on = !self.workspace.crop().mode;
                self.panel_view(&mut Vec::new()).disarm_tools();
                self.editor.set_crop_mode(&mut self.workspace, on);
            }
            // Enter commits the crop (LR, D13): the box is already live in
            // recipe.crop — committing is dropping the grab and disarming.
            if do_crop_commit {
                self.workspace.crop_mut().drag = None;
                self.editor.set_crop_mode(&mut self.workspace, false);
            }
            if crop_nudge != (0.0, 0.0)
                && let Some(c) = self.editor.doc().recipe.crop
            {
                // Keyboard twin of the move drag (D13): 0.5% of the frame per
                // press (Shift ×10 via the multiplier above), clamped
                // in-frame; a full-frame box has no room and stays put — and
                // NO box at all is a no-op: materialising an identity crop
                // from None lit a false ● and stamped HasCrop into the next
                // XMP (M19).
                let c0 = [c.left, c.top, c.right, c.bottom];
                let (w, h) = (c0[2] - c0[0], c0[3] - c0[1]);
                let nl = (c0[0] + crop_nudge.0 * 0.005).clamp(0.0, 1.0 - w);
                let nt = (c0[1] + crop_nudge.1 * 0.005).clamp(0.0, 1.0 - h);
                let next = Some(autoshade::recipe::Crop {
                    left: nl,
                    top: nt,
                    right: nl + w,
                    bottom: nt + h,
                });
                if self.editor.doc().recipe.crop != next {
                    self.editor.doc_mut().recipe.crop = next;
                    self.editor.session_mut().dirty = true; // histogram/clipping follow the crop
                }
            }
            if do_crop_grid {
                self.workspace.crop_mut().grid = (self.workspace.crop().grid + 1) % 3;
            }
            // LR tool keys: each mirrors its own button exactly
            // (disarm-then-arm; a second press exits the tool).
            if do_wb && self.editor.photo().src_path.is_some() {
                let on = !self.workspace.tools().wb_picking;
                self.panel_view(&mut Vec::new()).disarm_tools();
                self.workspace.tools_mut().wb_picking = on;
                if on {
                    self.workspace.set_status(tr(self.lang, K_APP_WB_EYEDROPPER_CLICK_A_SPOT_THAT_SHOULD_BE_NEUTRA).into());
                }
            }
            if do_heal && self.editor.photo().src_path.is_some() {
                let on = !self.workspace.tools().clone_mode;
                self.panel_view(&mut Vec::new()).disarm_tools();
                self.workspace.tools_mut().clone_mode = on;
            }
            if do_brush && self.editor.photo().src_path.is_some() {
                self.commands().toggle_brush_key();
            }
            if do_linear && self.editor.photo().src_path.is_some() {
                let armed = matches!(self.workspace.masks().placing_mask, Some((MaskKind::Linear, PlaceTarget::NewMask)));
                self.panel_view(&mut Vec::new()).disarm_tools();
                if !armed {
                    self.workspace.masks_mut().placing_mask = Some((MaskKind::Linear, PlaceTarget::NewMask));
                    self.workspace.set_status(tr(self.lang, K_APP_DRAG_ON_THE_IMAGE_TO_DRAW_A_LINEAR_GRADIENT_STAR).into());
                }
            }
            if do_radial && self.editor.photo().src_path.is_some() {
                let armed = matches!(self.workspace.masks().placing_mask, Some((MaskKind::Radial, PlaceTarget::NewMask)));
                self.panel_view(&mut Vec::new()).disarm_tools();
                if !armed {
                    self.workspace.masks_mut().placing_mask = Some((MaskKind::Radial, PlaceTarget::NewMask));
                    self.workspace.set_status(tr(self.lang, K_APP_DRAG_ON_THE_IMAGE_TO_DRAW_A_RADIAL_ELLIPTICAL_AR).into());
                }
            }
            // \ latches the Before view, Y flips the comparison layout (LR).
            if do_before {
                self.workspace.view_mut().before = !self.workspace.view().before;
            }
            if do_compare {
                self.workspace.view_mut().mode = if self.workspace.view().mode == ViewMode::SideBySide {
                    ViewMode::AfterOnly
                } else {
                    ViewMode::SideBySide
                };
            }
            if do_panels {
                self.workspace.ui_mut().panels_hidden = !self.workspace.ui().panels_hidden;
                self.workspace.ui_mut().defocus_next = true; // undo egui's Tab focus traversal next frame
            }
            if brush_delta != 0.0 {
                // Proportional, as Lightroom's [ / ]: a fixed 4 px step was
                // fifty presses across the range.
                let now = ctx.input(|i| i.time);
                let factor = if brush_delta > 0.0 { 1.15 } else { 1.0 / 1.15 };
                self.workspace.set_brush((self.workspace.masks().brush * factor).round(), now);
            }
            if do_cheatsheet {
                self.workspace.ui_mut().show_shortcuts = !self.workspace.ui().show_shortcuts;
            }
            // Esc leaves whatever on-image tool is active (the universal
            // editor exit) — the transient windows own their Esc in the
            // pre-tier above; painted canvases/samples stay for resuming.
            if do_escape
                && (self.workspace.tool_armed() || self.workspace.overlay().region_drag.is_some() || self.workspace.view().before)
            {
                // disarm_tools also kills the transient gesture anchors — a
                // grab or stroke surviving Esc used to hijack the next drag.
                // The \-latched Before view exits here too: it locks every
                // tool out, and Esc is the universal way back (M21).
                self.panel_view(&mut Vec::new()).disarm_tools();
                self.workspace.overlay_mut().region_drag = None;
                self.workspace.view_mut().before = false;
                self.workspace.set_status(tr(self.lang, K_APP_EXITED_THE_CURRENT_TOOL_ESC).into());
            }
            if do_overlay {
                self.workspace.overlay_mut().show_mask_overlay = !self.workspace.overlay().show_mask_overlay;
                self.workspace.overlay_mut().overlay_stale = true;
            }
            if do_clip {
                // Instant: rebuilt from the retained frame, no redevelop (J).
                self.panel_view(&mut Vec::new()).toggle_clipping(ctx);
            }
            if nav != 0 && !self.workspace.busy() && !self.workspace.gal().gallery.is_empty() {
                // Nothing selected yet: either arrow ENTERS the gallery at its
                // first photo (the old Right-arrow default skipped index 0).
                let next = match self.workspace.gal().selected {
                    Some(i) => (i as i32 + nav).clamp(0, self.workspace.gal().gallery.len() as i32 - 1),
                    None => 0,
                };
                if Some(next as usize) != self.workspace.gal().selected {
                    // Keyboard walking must keep the highlight on screen — the
                    // gallery scrolls to it next frame (clicks don't set this:
                    // a clicked row is visible by definition).
                    self.workspace.gal_mut().gallery_scroll_to = Some(next as usize);
                    self.commands().open_gallery_index(next as usize);
                }
            }
            // Zoom keys act on the open photo's canvas. Fit/1:1 mirror their
            // buttons exactly (1:1 via the per-frame zoom_one_to_one twin);
            // +/− step about the view centre — pan holds, view_uv reclamps.
            // All three set the TARGET only: the glide in update() carries
            // `zoom` there, and +/− compound on the target so a key roll
            // accumulates instead of fighting the animation.
            if self.editor.photo().src_path.is_some() {
                if do_fit {
                    self.workspace.view_mut().zoom_target = 1.0;
                    self.workspace.view_mut().pan = vec2(0.5, 0.5);
                }
                if do_one {
                    self.workspace.view_mut().zoom_target = self.workspace.view().one_to_one.clamp(1.0, 12.0);
                }
                if zoom_key != 0.0 {
                    self.workspace.view_mut().zoom_target =
                        (self.workspace.view().zoom_target * 1.25f32.powi(zoom_key as i32)).clamp(1.0, 12.0);
                }
            }
            // Ctrl+C mirrors ⎘ Copy recipe (same guards + the pending-rename
            // flush); Ctrl+V mirrors ⇩ Paste to selected — and like the
            // disabled buttons, a press that would not act stays silent.
            if do_copy && self.editor.photo().src_path.is_some() && !self.workspace.busy() {
                // Every name box at once (the 10+1 boundary rule): a name
                // typed into a mask row, a version row or a variant card and
                // never blurred still lands before anything else in the app
                // acts.
                self.commands().commit_pending_names();
                self.workspace.genai_mut().copied = Some(self.editor.doc().recipe.clone());
                self.workspace.genai_mut().copied_from = self.editor.photo().src_path.clone();
                *self.workspace.status_mut() = tr(
                    self.lang,
                    K_APP_RECIPE_COPIED_CTRL_CLICK_TO_PICK_SEVERAL_THEN_PA,
                )
                .to_string();
            }
            if do_paste && !self.workspace.busy() && self.workspace.genai().copied.is_some() && !self.workspace.gal().multi_sel.is_empty() {
                self.commands().start_paste();
            }
        }

        // Drag & drop: dropping a photo opens it, a folder opens the library.
        // Ignored entirely while the quit-confirm layer is up — a drop must
        // not mutate the very state the user is deciding whether to save
        // (the shortcut block is gated on the same condition).
    }



    /// Variant-strip panel height. Derivation: an 84 px card column (52 px
    /// thumb, 6 px item_spacing, and a 26 px label row — egui rows are at
    /// least interact_size.y tall), 8 px of air above and below the column
    /// (variant_strip top-pads it into center), and the panel frame's
    /// 2+2 px vertical margins. One authority — the strip geometry test
    /// lays the real panel out at this same constant and asserts the two
    /// airs are EQUAL.
    pub(crate) const VARIANT_STRIP_H: f32 = 104.0;

    // The frame draws on the command context; the root keeps one delegating
    // entry per phase so `run_frame` and the headless tests keep their call
    // shape (`app.upd_top_bar(ctx)`), and this impl does not grow a body per
    // overlay.
    fn upd_dropped_and_title(&mut self, ctx: &Ctx) {
        self.commands().upd_dropped_and_title(ctx);
    }

    pub(crate) fn upd_top_bar(&mut self, ctx: &Ctx) {
        self.commands().upd_top_bar(ctx);
    }

    pub(crate) fn upd_status_bar(&mut self, ctx: &Ctx) {
        self.commands().upd_status_bar(ctx);
    }

    #[cfg(test)]
    pub(crate) fn controls_column(&mut self, ui: &mut Ui) {
        self.commands().controls_column(ui);
    }

    pub(crate) fn upd_strips_and_side_panels(&mut self, ctx: &Ctx) {
        self.commands().upd_strips_and_side_panels(ctx);
    }

    pub(crate) fn upd_central(&mut self, ctx: &Ctx) {
        self.commands().upd_central(ctx);
    }

    pub(crate) fn upd_sheets(&mut self, ctx: &Ctx) {
        self.commands().upd_sheets(ctx);

    }
}

impl Default for AutoShadeApp {
    fn default() -> Self {
        Self {
            editor: Editor::default(),
            workspace: WorkspaceView::default(),
            services: Services::default(),
            aip: AiPrefs::default(),
            // 0.65 — braver than the 0.5 calibration point, by user decision
            // (2026-08-17 ⑦). Same one-definition rule as above: the constant is
            // the lib's own `GradeStrength::DEFAULT`.
            // OFF: an extra image on a paid call is the user's opt-in, never a
            // default (the same rule `fit_ai_judge` follows).
            // OFF, same rule and a bigger bill (up to 17 calls per analyze).
            lang: Lang::En,
            #[cfg(test)]
            seams: TestSeams::default(),
        }
    }
}

impl AutoShadeApp {
    /// One frame of the app: the whole of `update` but the prefs flush, so a
    /// headless test can drive it with a bare `Context`.
    pub(crate) fn run_frame(&mut self, ctx: &Ctx) {
        // First frame: re-issue the OS-titlebar theme. install_theme sends it
        // from the creation closure, but viewport commands sent before the
        // event loop runs are dropped — a FIRST-ever launch (no prefs, so no
        // second install) kept the OS-default titlebar over dark chrome.
        if !std::mem::replace(&mut self.workspace.ui_mut().os_theme_synced, true) {
            ctx.send_viewport_cmd(ViewportCommand::SetTheme(self.workspace.ui().theme.system()));
            // …and say which system faces the font chain turned away.
            // `install_fonts` runs before this app exists and can only reach
            // stderr, which a windowed build does not have — so the first
            // frame is the earliest moment a user can be told at all. A
            // refusal is a decision this build made about a font that IS
            // installed, which is a different fact from the tofu warning the
            // gallery raises about names it cannot draw.
            for font in refused_system_fonts() {
                let t = trf(
                    self.lang,
                    K_APP_A_SYSTEM_FONT_WAS_FOUND_BUT_NOT_LOADED_FILE_NAME,
                    &[("font", font)],
                );
                self.workspace.toast(ToastKind::Error, t);
            }
        }
        self.poll_workers(ctx);
        // What quitting would cost, published for the one caller that cannot
        // reach `self`: macOS's `applicationShouldTerminate:`, which AppKit
        // invokes from outside this loop for ⌘Q, the App menu's Quit, the
        // Dock's Quit and log-out (see `quit.rs`). On every other platform
        // this is a cheap store nothing reads — the state machine is compiled
        // and tested everywhere so the Windows battery gates it.
        crate::quit::publish(self.editor.quit_state(&self.workspace, ));
        // A vetoed quit becomes an ordinary close request, so ⌘Q lands in the
        // SAME guard below as the title-bar ✕ rather than a parallel dialog
        // that would have to be kept in step with it.
        if crate::quit::take_close_request() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        // Window-close guard: the unsaved-edit protection (● + nav_stash)
        // used to stop at photo switching — the title-bar ✕ dropped the open
        // photo's uncommitted develop AND every stashed one with no prompt.
        // An in-app confirm layer (egui Window, never an OS dialog) offers
        // save-all / discard / cancel; the guard re-checks on the way out, so
        // both quit buttons work by making the state genuinely clean.
        if ctx.input(|i| i.viewport().close_requested()) {
            // A typed-but-uncommitted mask or CARD rename must COUNT as
            // unsaved work: commit them before the dirty check below — an
            // otherwise clean recipe used to close without a prompt and the
            // rename died with the window (U10). A version rename is not
            // unsaved work in the same sense (it writes straight to its own
            // advisory sidecar), but it must not die with the window either,
            // so the last boundary before the quit decision flushes the lot.
            self.commands().commit_pending_names();
            self.commands().fork_edited_card();
            let unsaved_open = self.editor.quit_guard_open_dirty();
            if self.workspace.busy() {
                // A running export / retouch / paid AI generation dies with
                // the process — the ✕ used to bypass every guard mid-flight.
                // Block and say why; close again once the worker lands.
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                let t = tr(self.lang, K_APP_AN_OPERATION_IS_STILL_RUNNING_WAIT_FOR_IT_TO_FIN).to_string();
                self.workspace.toast(ToastKind::Error, t);
            } else if !self.workspace.open().discard_requested
                && (unsaved_open
                    || !self.workspace.open().nav_stash.is_empty()
                    || self.editor.inactive_dirty_variants(&self.workspace, ) > 0)
            {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                self.workspace.ui_mut().confirm_quit = true;
                // Surrender any widget focus NOW: the layer's Enter-saves
                // default is gated on "no widget focused", and a text field
                // that kept focus from before the close request silently
                // disabled the promised keyboard default. (Tab inside the
                // dialog can still focus its buttons — the gate then
                // correctly routes Enter to the focused control.)
                if let Some(id) = ctx.memory(|m| m.focused()) {
                    ctx.memory_mut(|m| m.surrender_focus(id));
                }
            }
        }
        if self.workspace.ui().confirm_quit {
            self.commands().confirm_quit_layer(ctx);
        }
        // Hover-to-preview is frame-scoped: take last frame's target; the mask
        // list re-sets it below if the cursor is still on a row. The diff is
        // checked right before the overlay refresh at the end of update().
        let hover_prev = self.workspace.overlay_mut().hover_mask.take();
        // The question a generation landing asks: BEFORE the shortcut tiers,
        // so its Esc / Enter are its own; never under the quit layer, which
        // outranks it.
        if !self.workspace.ui().confirm_quit {
            self.commands().fit_prompt_layer(ctx);
        }

        self.upd_shortcuts(ctx);
        self.workspace.apply_zoom_glide(ctx);
        self.upd_dropped_and_title(ctx);

        self.upd_top_bar(ctx);

        self.upd_status_bar(ctx);

        self.upd_strips_and_side_panels(ctx);
        // "Keep skin natural" (Portraits, `panels/portrait.rs`): whatever this
        // frame wrote into the recipe — a slider, a paste, an AI landing —
        // meets the skin limits here, before the fork, the develop and the
        // undo commit below read it.
        self.editor.guard_portrait_skin(&mut self.workspace);
        // The immutability rule (2026-09-13): whatever this frame wrote into
        // the live recipe — a slider, a shortcut, a landing — continues on
        // an ✎ card when the active card is the pristine ✨ image, BEFORE the
        // turn sync, the Before rebuild, the develop dispatch and the undo
        // commit below see it.
        self.commands().fork_edited_card();
        // The plates follow `recipe.quarter_turns` (R27) BEFORE the Before
        // rebuild and the redevelop dispatch below, so a turn — however it
        // arrived: the toolbar, an undo, a variant switch, a version load —
        // lands in the same frame it was asked for. Returns on a `u8` compare
        // when nothing turned, which is every frame but the one.
        self.editor.sync_base_turns(&mut self.workspace, self.lang, ctx);
        // Reset re-stamping a legacy photo, a paste, or a restore would
        // otherwise leave the compare against a stale starting point. One
        // cheap LUT-only develop, and only when the curve actually changed.
        if self.editor.photo().src_path.is_some()
            // NOT base-present: an InPlace master legitimately renders Before
            // under the canvas curve, and gating on v.base froze its Before
            // against Reset / undo / paste forever. A Generated canvas
            // recipe's curve is empty by construction, so this compare is
            // simply quiet there.
            && !self.editor.active_on_ai_pixels()
            && self.editor.doc().recipe.start_only() != self.workspace.open().before_start
            && let Some(b) = self.editor.photo().base_preview.clone()
        {
            let start = self.editor.doc().recipe.start_only();
            self.editor.set_before(&mut self.workspace, ctx, &b, &start);
        }
        // A held-still pointer still needs a repaint to receive the result.
        if self.editor.session().dirty && !self.editor.scheduler().inflight {
            self.commands().start_redevelop();
        }
        if self.editor.scheduler().inflight {
            ctx.request_repaint();
        }
        // The mask coverage overlay follows develop / selection / toggle /
        // hover (a changed hover target includes "left the list entirely").
        if self.workspace.overlay().hover_mask != hover_prev {
            self.workspace.overlay_mut().overlay_stale = true;
        }
        if std::mem::take(&mut self.workspace.overlay_mut().overlay_stale) {
            self.commands().refresh_mask_overlay(ctx);
        }

        self.upd_central(ctx);

        self.upd_sheets(ctx);
        // Gated like drop PROCESSING is: while a modal layer is up, drops are
        // deliberately discarded — promising "Drop to open" there was a lie.
        if !self.workspace.modal_up() && ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let painter = ctx.layer_painter(LayerId::new(
                Order::Foreground,
                Id::new("drop_overlay"),
            ));
            let rect = ctx.content_rect();
            // Chrome-side veil → the chrome accent's family (see the colour
            // rule at ACCENT), dimmed to 0.29 so the white line reads on it
            // in every theme.
            let a = self.workspace.ui().theme.colors().accent_fill;
            let dim = |v: u8| (f32::from(v) * 0.29).round() as u8;
            painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(dim(a.r()), dim(a.g()), dim(a.b()), 150));
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                tr(self.lang, K_APP_DROP_TO_OPEN),
                FontId::proportional(28.0),
                Color32::WHITE,
            );
        }

        // Transient toasts (bottom-right). Errors linger longer than
        // successes, long texts longer than short ones (Toast::ttl), a click
        // dismisses early, and the last 300 ms fade instead of blinking out.
        self.workspace.chrome_mut().toasts.retain(|t| t.born.elapsed() < t.ttl());
        if !self.workspace.chrome().toasts.is_empty() {
            let mut dismiss: Option<usize> = None;
            let mut fading = false;
            Area::new(Id::new("toasts"))
                .anchor(Align2::RIGHT_BOTTOM, vec2(-12.0 + MOTION_TOAST_SLIDE, -40.0))
                .constrain(false)
                .order(Order::Foreground)
                .show(ctx, |ui| {
                    let c = self.workspace.ui().theme.colors();
                    // Right-aligned rows: a toast slides in from the right
                    // edge by a shrinking right-hand gap.
                    ui.with_layout(Layout::top_down(Align::Max), |ui| {
                    for (idx, t) in self.workspace.chrome().toasts.iter().enumerate() {
                        let (bg, fg) = match t.kind {
                            ToastKind::Success => (c.toast_ok_bg, c.toast_ok_fg),
                            ToastKind::Error => (c.toast_err_bg, c.toast_err_fg),
                        };
                        let left = t.ttl().saturating_sub(t.born.elapsed());
                        let arrive = ease_out(t.born.elapsed().as_secs_f32() / MOTION_TOAST_IN_S);
                        let fade = (left.as_millis() as f32 / 300.0).min(1.0) * arrive;
                        fading |= fade < 1.0;
                        // Elevated pill: the theme's popup shadow + a faint
                        // text-tinted hairline lift it off the photo — a flat
                        // fill over arbitrary canvas content had no edge at
                        // all where the photo matched the toast colour.
                        let mut shadow = ui.visuals().popup_shadow;
                        shadow.color = shadow.color.gamma_multiply(fade);
                        let r = ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        // The area's right edge sits MOTION_TOAST_SLIDE past
                        // the old anchor (unconstrained, so it may leave the
                        // window); a settled toast keeps that gap, an
                        // arriving one starts with none — off to the right.
                        ui.gap(arrive * MOTION_TOAST_SLIDE);
                        Frame::NONE
                            .fill(bg.gamma_multiply(fade))
                            .stroke(Stroke::new(1.0, fg.gamma_multiply(0.35 * fade)))
                            .shadow(shadow)
                            .corner_radius(CornerRadius::same(RADIUS_MD.round() as u8))
                            .inner_margin(Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                // Named token (#14a): this 420 is a WRAPPED
                                // paragraph's measure, not the prompt-field
                                // ceiling that happens to share the number —
                                // see theme.rs on why they are two constants.
                                ui.set_max_width(TOAST_W_MAX);
                                // `.wrap()` explicitly: this ui is a child of
                                // the right_to_left row above, and a label in
                                // a horizontal layout EXTENDS by default
                                // (egui 0.29 `Ui::wrap_mode`), so a long error
                                // text ignored the measure and ran past the
                                // window's edge as one line.
                                ui.add(
                                    Label::new(Text::new(&t.text).color(fg.gamma_multiply(fade)))
                                        .wrap(),
                                );
                            })
                            .response
                            .interact(Sense::click())
                        })
                        .inner;
                        if r.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        if r.clicked() {
                            dismiss = Some(idx);
                        }
                        ui.gap(SPACE_MD);
                    }
                    });
                });
            if let Some(i) = dismiss {
                self.workspace.chrome_mut().toasts.remove(i);
            }
            // Keep repainting so expiry doesn't wait for the next input
            // event — per frame while a fade is running, else a coarse poll.
            if fading {
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }

        // Land a finished edit gesture (slider release, AI Analyze, Reset) into
        // the undo history — once per gesture, after all controls are read.
        self.editor.commit_if_settled(&self.workspace, ctx);
        // LAST, after every panel had its chance to spawn (L12-3): evaluated
        // at the top of update (inside poll_workers) the gate judged a frame
        // whose spawns had not happened yet, so the FIRST batch of workers
        // each frame started with no completion pump armed and their results
        // sat in the channel until the next input event.
        self.workspace.pump_repaint_gate(ctx);
    }
}

impl AutoShadeApp {
    /// The persisted preferences snapshot — what the storage plugin writes
    /// and what the launch restores. `view.rs` only speaks the storage
    /// plugin's API; what persists is this file's business.
    pub(crate) fn prefs_snapshot(&self) -> Prefs {
        Prefs {

            prefs_era: PREFS_ERA,
            gallery_dir: self.workspace.gal().gallery_dir.clone(),
            guide: self.aip.guide,
            style_strength: self.aip.style_strength,
            grade_strength: self.aip.grade_strength,
            fit_strength: self.aip.fit_strength,
            send_style_ref_image: self.aip.send_style_ref_image,
            deep_think: self.aip.deep_think,
            style_embed: self.aip.style_embed,
            style_describe: self.aip.style_describe,
            style_src_dir: self.aip.style_src_dir.clone(),
            style_raw_dir: self.aip.style_raw_dir.clone(),
            direction_adherence: self.aip.direction_adherence,
            // Both written: exp_format is the truth, save_jpeg keeps a
            // pre-阶段4 build reading this prefs file sane.
            save_jpeg: self.workspace.exp().format == ExportFormat::Jpeg,
            exp_format: self.workspace.exp().format.pref_code(),
            exp_dest: self.workspace.exp().dest.pref_code(),
            last_export_dir: self.workspace.exp().last_dir.clone(),
            save_denoise: self.workspace.exp().save_denoise,
            save_denoise_strength: self.workspace.exp().save_denoise_strength,
            denoise_strength: self.workspace.exp().denoise_strength,
            baked_denoise_strength: self.workspace.exp().baked_denoise_strength,
            baked_save_denoise_strength: self.workspace.exp().baked_save_denoise_strength,
            zoned_fit: self.workspace.fit().zoned_fit,
            zoned_four_regions: self.workspace.fit().zoned_four_regions,
            fit_ai_judge: self.workspace.fit().fit_ai_judge,
            // R23-6 D: the deep reverse-fit toggle persists like every
            // other paid opt-in. `fit_ref` deliberately does NOT — it
            // names one file for one photo (see the field).
            fit_deep: self.workspace.fit().fit_deep,
            reimagine_retry: self.workspace.fit().reimagine_retry,
            ask_fit_after_generate: self.workspace.fit().ask_fit_after_generate,
            adjust_quality: self.workspace.genai().adjust_quality,
            view_mode: self.workspace.view().mode,
            exp_long_edge: self.workspace.exp().long_edge,
            exp_sharpen: self.workspace.exp().sharpen,
            exp_quality: self.workspace.exp().quality,
            exp_space: self.workspace.exp().space,
            preview_edge: self.workspace.genai().preview_edge,
            show_clipping: self.workspace.chrome().show_clipping,
            lang: self.lang,
            theme: self.workspace.ui().theme,
            accent: self.workspace.ui().accent,
        }
    }
}
