//! Background workers: spawn, master loads, the Msg pump.

use super::*;

impl AutoShadeApp {

    pub(crate) fn poll_workers(&mut self, ctx: &egui::Context) {
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
                Msg::Exported(res) => {
                    // Normal's Save group words its own landing (`normal.rs`).
                    let plain = res
                        .as_ref()
                        .ok()
                        .and_then(|outcome| self.workspace.normal_delivery_words(lang, outcome));
                    self.workspace.exp_mut().batch_progress = None; // the bar belongs to ONE batch run
                    match export_workflow::land(lang, res) {
                        export_workflow::ExportLanding::Failed(e) => {
                            self.workspace.normal_mut().delivering = false;
                            self.workspace.fail(tr(lang, K_GUI_EXPORT_FAILED), e);
                        }
                        export_workflow::ExportLanding::Single { relooked, line, error } => {
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
                            if error {
                                self.workspace.set_status(line.clone());
                                self.workspace.toast(ToastKind::Error, line);
                            } else {
                                self.workspace.done(line);
                            }
                        }
                        export_workflow::ExportLanding::Batch { ok, dest, line, error } => {
                            // Where the batch actually DELIVERED becomes the
                            // remembered destination — here, not at the dialog
                            // (R22 M1): a folder inside the photo library is
                            // refused per photo, so a batch that picked one
                            // lands nothing and must not be remembered. `ok > 0`
                            // is the proof that at least one file is there.
                            if ok > 0 {
                                self.workspace.exp_mut().last_dir = Some(dest);
                            }
                            if error {
                                // A partial failure keeps the error channel,
                                // exactly as the worker-side bail! did.
                                self.workspace.fail(tr(lang, K_GUI_EXPORT_FAILED), anyhow::anyhow!("{line}"));
                            } else {
                                self.workspace.done(line);
                            }
                        }
                    }
                    if let Some(plain) = plain {
                        self.workspace.reword_landing(plain);
                    }
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


    }
