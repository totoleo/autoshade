//! The AI start/landing commands and the modal dialogs: the analyze and
//! reverse-fit spawns, the reference-library build/info starts, the legacy
//! import, the cancel token, and the fit-prompt / quit-confirm layers. Same
//! `AutoShadeApp` impl as `actions.rs`, split by domain.

use autoshade::advisor::{hint_action, FitAction};
use crate::*;

impl Commands<'_> {

    /// The status-bar ✕: stop the running retouch/generative task. The UI
    /// unblocks NOW; the worker sees the flag at its next checkpoint and its
    /// late result arrives under a stale epoch — discarded, never applied.
    pub(crate) fn cancel_generative(&mut self) {
        // The cancelled analysis' late result has no claim to win.
        logic::look::cancel(&mut self.workspace.tools_mut().look);
        if let Some(flag) = self.workspace.tools_mut().gen_cancel.take() {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            self.workspace.tools_mut().gen_epoch = self.workspace.tools().gen_epoch.wrapping_add(1);
            self.workspace.set_busy(false);
            // Honest about BOTH shapes. A generative worker polls the flag
            // between stream events and really does stop; `produce_recipe`
            // takes no flag at all, so an Analyze keeps running — and keeps
            // billing — until it answers or hits its deadline. Claiming a
            // checkpoint that does not exist for the path the user is most
            // likely cancelling is the kind of false status this project
            // treats as a defect.
            *self.workspace.status_mut() = tr(
                self.lang,
                K_ACTIONS_CANCELLED_THE_APP_IS_FREE_AGAIN_AND_THE_LATE_RES,
            )
            .into();
        }
    }

    /// Raise the fit prompt after a whole-frame reimagine landed and the strip
    /// switched to its ✨ card. Not over a running job (the answer could not
    /// start), not once the user said not to ask, and not without the
    /// negative's preview [`Self::start_fit`] reads.
    pub(crate) fn offer_fit_prompt(&mut self, ctx: &egui::Context, divergence: f32) {
        if self.workspace.busy()
            || !self.workspace.fit().ask_fit_after_generate
            || self.editor.photo().source_preview.is_none()
        {
            return;
        }
        let Some(target) = self.editor.active_variant()
            .filter(|v| v.kind == VariantKind::Generated && v.origin.is_some())
            .map(|v| v.id.clone())
        else {
            return;
        };
        // One question at a time: a newer generation takes the open one over
        // — the strip already stands on its card — and keeps what the user
        // picked in it.
        if let Some(p) = &mut self.workspace.fit_mut().fit_prompt {
            p.target = target;
            p.divergence = divergence;
            return;
        }
        // The ▣ original first (the user's wording: 默认原图), not Automatic,
        // which solves from a ◈ / ▦ master whenever the strip holds one.
        let from = self.editor.original_index().map(|i| self.editor.doc().variants[i].id.clone()).filter(|id| !id.is_empty());
        self.workspace.fit_mut().fit_prompt = Some(FitPrompt { target, divergence, from, dont_ask: false });
        // The layer's Enter waits for "no widget focused" — surrendered now,
        // as the quit layer does when it opens.
        if let Some(id) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
    }

    /// The fit prompt (v1.17.0, user directive): after「AI generate」, ask
    /// whether to reverse-fit the new ✨ card, and from which negative. It
    /// only hands over to the fold's own verb — 「Reverse-fit」 sets the
    /// fold's source and calls [`Self::start_fit`] — so the fit it starts IS
    /// the panel button's; the strength and the switches stay the fold's
    /// controls (the AI review is paid, and its cost is named there), and the
    /// layer only states them.
    pub(crate) fn fit_prompt_layer(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let Some(mut prompt) = self.workspace.fit_mut().fit_prompt.take() else {
            return;
        };
        let (automatic, sources) = self.fit_source_menu();
        let to = self.editor.doc().variants
            .iter()
            .position(|v| v.id == prompt.target)
            .map(|i| trf(lang, K_ACTIONS_TO, &[("card", &self.numbered_card_label(i))]));
        let fidelity = crate::i18n::text(lang, &logic::retouch::reimagine_fidelity(prompt.divergence));
        let on_off = |on: bool| if on { tr(lang, K_ACTIONS_ON) } else { tr(lang, K_ACTIONS_OFF) };
        let in_force = trf(
            lang,
            K_ACTIONS_REVERSE_FIT_STRENGTH_LOCAL_AREAS_AI_REVIEW_SET_I,
            &[
                ("pct", &format!("{:.0}", self.aip.fit_strength * 100.0)),
                ("zoned", on_off(self.workspace.fit().zoned_fit)),
                ("review", on_off(self.workspace.fit().fit_ai_judge)),
            ],
        );
        let mut open = true;
        let mut fit = false;
        let mut later = false;
        // The quit layer's keyboard grammar: Esc = the safe escape (nothing
        // changes), Enter = the primary — unless a control holds focus, which
        // then owns its own Enter.
        let widget_focused = ctx.memory(|m| m.focused()).is_some();
        ctx.input_mut(|i| {
            if Chord::ESCAPE.consume(i) {
                later = true;
            }
            if !widget_focused && Chord::ENTER.consume(i) {
                fit = true;
            }
        });
        // And the pointer (D13): the strip and the sliders behind it must not
        // change the card the answer is about.
        egui::Area::new(egui::Id::new("fit_prompt_scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |ui| {
                let r = ctx.content_rect();
                ui.allocate_response(r.size(), egui::Sense::click_and_drag());
                ui.painter().rect_filled(r, 0.0, egui::Color32::from_black_alpha(96));
            });
        egui::Window::new(tr(lang, K_ACTIONS_REVERSE_FIT_THIS_AI_RESULT))
            .id(egui::Id::new("fit_prompt"))
            .order(egui::Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(fit_explainer(lang));
                // The landing's own reading, without its ` · ` joint.
                ui.label(egui::RichText::new(fidelity.trim_start_matches(" · ")).small().weak());
                if let Some(to) = &to {
                    ui.label(egui::RichText::new(to).small().weak());
                }
                ui.horizontal(|ui| {
                    ui.label(tr(lang, K_ACTIONS_FROM));
                    let shown = match prompt.from.as_deref() {
                        Some(id) => sources.iter().find(|(s, _)| s == id).map(|(_, l)| l.clone()),
                        None => None,
                    }
                    .unwrap_or_else(|| automatic.clone());
                    // The fold's menu, the ▣ original first: Automatic stays
                    // offered, after the cards it chooses among.
                    egui::ComboBox::from_id_salt("fit_prompt_from").selected_text(shown).show_ui(ui, |ui| {
                        for (id, label) in &sources {
                            let on = prompt.from.as_deref() == Some(id.as_str());
                            if ui.selectable_label(on, label).clicked() {
                                prompt.from = Some(id.clone());
                            }
                        }
                        if ui.selectable_label(prompt.from.is_none(), &automatic).clicked() {
                            prompt.from = None;
                        }
                    });
                });
                ui.label(egui::RichText::new(&in_force).small().weak());
                ui.checkbox(&mut prompt.dont_ask, tr(lang, K_ACTIONS_DON_T_ASK_AGAIN_AFTER_GENERATING))
                    .on_hover_text(tr(lang, K_ACTIONS_TURN_IT_BACK_ON_IN_THE_AI_PANEL_S_REVERSE_FIT_FO));
                ui.add_space(SPACE_MD);
                // The safe answer first, the primary tinted and last — the
                // quit layer's order.
                ui.horizontal(|ui| {
                    later |= action(ui, true, tr(lang, K_ACTIONS_NOT_NOW)).on_hover_text("Esc").clicked();
                    fit |= primary(ui, true, tr(lang, K_ACTIONS_REVERSE_FIT)).on_hover_text("Enter").clicked();
                });
            });
        if !open {
            later = true;
        }
        if !(fit || later) {
            self.workspace.fit_mut().fit_prompt = Some(prompt);
            return;
        }
        if prompt.dont_ask {
            self.workspace.fit_mut().ask_fit_after_generate = false;
        }
        // The answer is about the card it asked about; the strip stands on it
        // (the scrim kept every other click away) unless a landing moved it.
        if fit && self.editor.active_variant().is_some_and(|v| v.id == prompt.target) {
            self.workspace.fit_mut().fit_from = prompt.from;
            // A reference file picked in the fold earlier wins `fit_target`
            // over the active card — the fit would solve toward it, not
            // toward the result the question named.
            self.workspace.fit_mut().fit_ref = None;
            self.start_fit();
        }
    }

    /// The quit-confirm layer (in-app egui window). Lists what quitting would
    /// lose; 「Save all & quit」 writes each pending develop exactly like
    /// Ctrl+S (explicit user save → no backup gate), 「Discard & quit」 makes
    /// the state clean so the close guard lets the next close through.
    pub(crate) fn confirm_quit_layer(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        // Everything quitting would lose: the stash + the open photo's canvas
        // (the live canvas outranks its own stale stash entry). Each entry
        // carries its pixel identity so 「Save all」 persists a baked retouch's
        // master link exactly like Ctrl+S would. (The bool is the
        // `pixels.json` flag — `VariantKind::on_ai_pixels`, the axis the
        // flag records: a ✨ card and its ✎ cards sit on the same raster.)
        let mut pending: Vec<PendingSave> = self.workspace.open().nav_stash
            .iter()
            .map(|(p, st)| PendingSave {
                photo: p.clone(),
                recipe: st.recipe.clone(),
                pixels: st.origin.clone().map(|o| (o, st.kind.on_ai_pixels())),
                strip: stash_strip_record(st),
            })
            .collect();
        // Live-canvas override — skipped while a photo OPEN is in flight:
        // src_path already points at the new photo but recipe/saved_recipe
        // still describe the old one, so acting on the pair would either drop
        // the new photo's stash entry or save the old recipe under the new
        // path. The stash (written by open_path just before the flight) is
        // the correct authority for that window. ONLY the open transition —
        // gating on plain `busy` let any background worker (Export…) empty
        // `pending`, and the empty-pending branch below then CLOSED the app,
        // losing a sole live unsaved canvas (background work still lands
        // under the dialog — the scrim below blocks only pointer input).
        if !self.workspace.open().open_in_flight && let Some(p) = self.editor.photo().src_path.clone() {
            let origin = self.editor.active_variant().and_then(|v| v.origin.clone());
            // The live canvas outranks its own stash entry WHOLESALE: displace
            // it even when the canvas is clean (the user reverted) — the loop
            // below would otherwise write the stale stashed edits to disk.
            pending.retain(|e| e.photo != p);
            // The strip counts as unsaved work of the OPEN photo too — a
            // clean canvas over an unpersisted strip must still be listed,
            // or Save-all had nothing to write and the guard's re-check
            // bounced the close forever (the v0.21 dead-button livelock).
            if self.editor.pending_save_gate_dirty() {
                let pix = origin.map(|o| (o, self.editor.active_on_ai_pixels()));
                pending.push(PendingSave {
                    photo: p,
                    recipe: self.editor.doc().recipe.clone(),
                    pixels: pix,
                    strip: self.editor.current_strip_record(),
                });
            }
        }
        // Background variants ride IN `pending` since v0.22: each entry
        // carries its photo's strip record, and Save-all persists it to
        // variants.json beside the develop. The count still gates the
        // empty-pending close below (defence-in-depth — a strip-dirty photo
        // is also pushed into `pending` above).
        let orphan_variants = self.editor.inactive_dirty_variants(&self.workspace, );
        if pending.is_empty() && orphan_variants == 0 {
            // Saved (or discarded) since the guard fired — nothing to protect.
            self.workspace.ui_mut().confirm_quit = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let mut open = true;
        let mut save_quit = false;
        let mut discard_quit = false;
        let mut cancel = false;
        // The global shortcut block is gated off while this layer is up, so
        // the promised keyboard grammar is honoured HERE: Esc = the safe
        // escape (always), Enter = the safe default (save everything) — but
        // only while NO button holds keyboard focus, or Enter on a focused
        // Cancel/Discard would fire Save instead of the focused control.
        let widget_focused = ctx.memory(|m| m.focused()).is_some();
        ctx.input_mut(|i| {
            if Chord::ESCAPE.consume(i) {
                cancel = true;
            }
            if !widget_focused && Chord::ENTER.consume(i) {
                save_quit = true;
            }
        });
        // A modal owns the pointer as well as the keyboard (D13): the scrim
        // swallows every click behind the dialog — sliders were still live
        // during the save-or-discard decision — and dims the stakes into
        // view. Background workers are unaffected (see open_in_flight above).
        egui::Area::new(egui::Id::new("confirm_quit_scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |ui| {
                let r = ctx.content_rect();
                ui.allocate_response(r.size(), egui::Sense::click_and_drag());
                ui.painter().rect_filled(r, 0.0, egui::Color32::from_black_alpha(96));
            });
        egui::Window::new(tr(lang, K_ACTIONS_UNSAVED_EDITS))
            .id(egui::Id::new("confirm_quit"))
            .order(egui::Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                if !pending.is_empty() {
                    ui.label(trf(
                        lang,
                        K_ACTIONS_PHOTO_S_HAVE_EDITS_THAT_WERE_NEVER_SAVED,
                        &[("n", &pending.len().to_string())],
                    ));
                }
                // Parent folder + stem: two DSC_0431 from two shoots must be
                // distinguishable in the one dialog where the stakes are real.
                egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                    for PendingSave { photo: p, .. } in &pending {
                        let folder = p
                            .parent()
                            .and_then(|d| d.file_name())
                            .and_then(|s| s.to_str())
                            .unwrap_or("");
                        ui.label(
                            egui::RichText::new(format!(
                                "{folder}/{}",
                                autoshade::pipeline::stem(p)
                            ))
                            .small()
                            .weak(),
                        );
                    }
                });
                if orphan_variants > 0 {
                    // Informational since v0.22: Save-all persists each
                    // photo's whole variant strip (variants.json), so these
                    // are rescued WITH their photo, not lost.
                    ui.add_space(SPACE_MD);
                    ui.label(
                        egui::RichText::new(trf(
                            lang,
                            K_ACTIONS_OTHER_VARIANT_S_HOLD_EDITS_SAVE_ALL_SAVES_EACH_P,
                            &[("n", &orphan_variants.to_string())],
                        ))
                        .small()
                        .weak(),
                    );
                }
                ui.add_space(SPACE_MD);
                // Safe escape far left, the destructive action fenced off by
                // space, the primary (save) tinted and last — a 4px slip from
                // Save must not land on an unrecoverable Discard.
                ui.horizontal(|ui| {
                    cancel |= action(ui, true, tr(lang, K_ACTIONS_CANCEL)).on_hover_text("Esc").clicked();
                    ui.add_space(18.0);
                    let warn = ui.visuals().warn_fg_color;
                    discard_quit = action(ui, true, egui::RichText::new(tr(lang, K_ACTIONS_DISCARD_QUIT)).color(warn))
                        .on_hover_text(tr(lang, K_ACTIONS_QUIT_WITHOUT_SAVING_THESE_EDITS_ARE_GONE_FOR_GOO))
                        .clicked();
                    save_quit |= primary(ui, true, tr(lang, K_ACTIONS_SAVE_ALL_QUIT))
                        .on_hover_text(tr(lang, K_ACTIONS_ENTER_SAVE_EVERY_LISTED_DEVELOP_THEN_QUIT))
                        .clicked();
                });
            });
        if save_quit {
            let mut failed: Option<String> = None;
            // Collected, not fatal — reported on the way out (see below).
            let mut xmp_warns: Vec<String> = Vec::new();
            let mut clear_warns: Vec<String> = Vec::new();
            for PendingSave { photo: p, recipe: r, pixels: pix, strip } in &pending {
                // ONE NoWait lock per photo across its whole persist compound
                // (the Ctrl+S pairing). A develop busy in another process
                // fails THIS photo — the quit bounces and reports below —
                // instead of freezing the dialog behind an unbounded Wait.
                let locked: std::io::Result<()> = autoshade::store::with_develop_lock(
                    p,
                    autoshade::store::DevelopLockMode::NoWait,
                    || {
                // Neutral + no pixel identity + trivial strip = Ctrl+S's
                // "clear my edits": WRITING neutral files here pinned the
                // existence-keyed ● badge that a direct Ctrl+S removes. A
                // non-trivial strip takes the save path below instead —
                // clear_develop would destroy the OTHER cards' work.
                if r.is_noop() && pix.is_none() && strip.is_none() {
                    // The store's ONE clear primitive (both surfaces): it takes
                    // the retired pixels.json.bak with it, which a bare unlink
                    // left behind for the next open to republish.
                    match autoshade::store::clear_develop(p) {
                        Ok(o) => {
                            // clear_develop took variants.json with it — the
                            // open photo's mirror must follow, or the guard's
                            // way-out re-check compares live-trivial against
                            // a stale record and bounces the close.
                            if self.editor.photo().src_path.as_deref() == Some(p.as_path()) {
                                self.editor.session_mut().hist.saved_strip = None;
                            }
                            // Cleared, but not MARKED — same channel as the XMP
                            // half: quitting silently would let a projection
                            // copied beside the RAW undo this clear unannounced.
                            if let Some(w) = o.marker_warning {
                                clear_warns.push(format!("{}: {w}", autoshade::pipeline::stem(p)));
                            }
                        }
                        Err(e) => {
                            failed = Some(format!("{}: {e}", autoshade::pipeline::stem(p)));
                        }
                    }
                    return Ok(());
                }
                // A GENERATED entry's recipe is the STRIPPED canvas form —
                // persisting it verbatim erased the RAW's calibration from
                // disk (the Analyze saver writes the calibrated form; a
                // master that later fails to decode then falls back to a
                // dark, uncorrected develop). Re-stamp from the same
                // saved-first source produce_recipe uses.
                let mut disk = r.clone();
                if pix.as_ref().is_some_and(|(_, g)| *g) {
                    // ONE snapshot for every half — independent reads could
                    // pair an OLD curve with a NEW profile if another surface
                    // published between them. The era stamp rides WITH the
                    // curve (the paste rule): this WRITES recipe.json, and a
                    // canvas-era stamp over a saved era-1 curve would launder
                    // it past every repair.
                    // Every calibration half, the start (v1.17.0) included,
                    // through the one stamper.
                    let cal = autoshade::pipeline::photo_calibration(p);
                    autoshade::pipeline::stamp_fit_calibration(&mut disk, cal);
                }
                // The WRITER's rule (Ctrl+S, api_xmp): a stashed canvas whose
                // open-time estimate failed still carries the washed pre-era
                // curve — repaired before it is persisted. Memo-cheap when
                // already repaired, and it also covers the generated arm
                // above, whose calibration snapshot may itself have adopted
                // an unrepaired curve when the store read hit an inability.
                // SAID on the loop's existing quitting-time channel — gated
                // on the RECIPE write, not the compound result below: the
                // pixel-link half can fail AFTER recipe.json already
                // published the re-estimated curve, and dropping the note
                // then hid a mutation that IS on disk (the Ctrl+S rule: the
                // disclosure travels with the mutation).
                let relook_note = autoshade::pipeline::repair_pre_era_base_curve(p, &mut disk);
                let generated = pix.as_ref().is_some_and(|(_, g)| *g);
                // The projection is a member of the same generation
                // (2026-09-13): written for a source develop, CLEARED for a
                // generated entry. Skipping the write alone (the rule below)
                // left a sidecar written for an earlier card standing — it
                // outlived the card and the recipe it projected, and the
                // open path restored it over the pristine generated pixels.
                let (xmp_member, xmp_outcome) = match autoshade::pipeline::xmp_projection_member(
                    p,
                    &disk,
                    !generated,
                    autoshade::diag::stderr(),
                ) {
                    Ok((m, note, _)) => (m, Ok(note)),
                    Err(e) => (autoshade::store::CommitMember::Keep, Err(e)),
                };
                // ONE single-generation commit per photo (the Ctrl+S rule,
                // L03): recipe + baked-pixels link + strip record land whole
                // or not at all — the background variants this dialog just
                // listed cannot outlive a save that tore between members.
                let res: anyhow::Result<()> = (|| {
                    let recipe_bytes = autoshade::pipeline::recipe_store_bytes(p, &disk, autoshade::diag::stderr())?;
                    let pixels = match pix {
                        Some((o, g)) => autoshade::store::CommitMember::Write(
                            autoshade::store::pixel_source_record_bytes(p, o, *g)?,
                        ),
                        None => autoshade::store::CommitMember::Clear,
                    };
                    let variants = match strip {
                        Some(rec) => autoshade::store::CommitMember::Write(
                            autoshade::store::variants_record_bytes(p, rec)?,
                        ),
                        None => autoshade::store::CommitMember::Clear,
                    };
                    autoshade::store::commit_develop(
                        p,
                        autoshade::store::DevelopCommit {
                            recipe: Some(recipe_bytes),
                            pixels,
                            variants,
                            xmp: xmp_member,
                        },
                    )?;
                    Ok(())
                })();
                match res {
                    Ok(()) => {
                        // The disclosure travels with the mutation (the
                        // Ctrl+S rule): the commit landed whole, so the
                        // re-estimated curve IS on disk.
                        if let Some(note) = relook_note.as_deref() {
                            eprintln!("⚠ {}: {note}", autoshade::pipeline::stem(p));
                        }
                        if self.editor.photo().src_path.as_deref() == Some(p.as_path()) {
                            self.editor.session_mut().hist.saved_strip = strip.clone();
                            // The FOURTH mirror of the single-save set
                            // (export.rs): pixels.json for the ACTIVE photo
                            // just committed, and without this the ● badge
                            // and the exit re-check compared against a stale
                            // master link — a just-saved retouch was
                            // re-reported as unsaved (L11-6).
                            self.editor.session_mut().hist.pixels_on_disk = pix.as_ref().map(|(o, _)| o.clone());
                        }
                    }
                    Err(e) => {
                        failed = Some(format!("{}: {e}", autoshade::pipeline::stem(p)));
                        return Ok(());
                    }
                }
                // NO XMP for a generated entry: the look lives in baked
                // pixels no parametric sidecar can reproduce, and writing one
                // here overwrote a real Lightroom sidecar with a lie — the
                // member above CLEARS the projection for such an entry in the
                // same generation. The recipe + pixels pair alone restores
                // faithfully.
                //
                // OUTSIDE the fatal result: the recipe write alone decides the
                // saved state (cross-surface rule), so a failed XMP projection
                // must not abort the remaining photos or refuse to quit over a
                // develop that IS durably saved.
                if autoshade::decode::is_raw(p) && !generated {
                    // The M6a mask-loss list is dropped HERE for the same
                    // reason as in the batch paste: `xmp_warns` renders under
                    // one fixed "projection(s) failed" sentence, which a
                    // lossy-but-successful projection is not. stderr carries
                    // the per-photo line from the builder.
                    match xmp_outcome {
                        Ok(None) => {}
                        // A regenerated (unmerged) sidecar loses LR-only
                        // properties — Save-all's warning list is exactly
                        // where that belongs (round-12 disclosure threading).
                        Ok(Some(n)) => {
                            xmp_warns.push(format!("{}: {n}", autoshade::pipeline::stem(p)));
                        }
                        Err(e) => {
                            xmp_warns.push(format!("{}: {e}", autoshade::pipeline::stem(p)));
                        }
                    }
                }
                        Ok(())
                    },
                );
                if let Err(e) = locked {
                    failed = Some(format!(
                        "{}: the photo is being changed by another AutoShade process ({e})",
                        autoshade::pipeline::stem(p)
                    ));
                }
                if failed.is_some() {
                    break;
                }
            }
            // ALL said regardless of a later photo's failure (the
            // travels-with-the-mutation rule the repair note above follows),
            // COUNTED (a single-slot accumulator reported only the last of
            // several same-kind failures), and worded per-photo so the
            // sentence stays true on the abort path — the old "develops
            // saved" claimed a completed batch even after a break.
            // stderr keeps the full list for logs; the USER-facing copy is
            // below — an eprintln alone vanished with the closing window,
            // which is exactly the silence W3 exists to end.
            if !xmp_warns.is_empty() {
                eprintln!(
                    "⚠ {} Lightroom XMP projection(s) failed for develop(s) that ARE saved: {}",
                    xmp_warns.len(),
                    xmp_warns.join("; ")
                );
            }
            if !clear_warns.is_empty() {
                eprintln!(
                    "⚠ {} clear(s) succeeded but could not be marked: {} — a sidecar \
                     beside the RAW may restore those edits on the next open",
                    clear_warns.len(),
                    clear_warns.join("; ")
                );
            }
            let mut quit_warns: Vec<String> = Vec::new();
            if !xmp_warns.is_empty() {
                quit_warns.push(trf(
                    lang,
                    K_ACTIONS_LIGHTROOM_XMP_PROJECTION_S_FAILED_THOSE_DEVELOPS,
                    &[("n", &xmp_warns.len().to_string()), ("detail", &logic::brief_list(&xmp_warns))],
                ));
            }
            if !clear_warns.is_empty() {
                quit_warns.push(trf(
                    lang,
                    K_ACTIONS_CLEAR_S_COULD_NOT_BE_MARKED_A_SIDECAR_BESIDE_THE,
                    &[("n", &clear_warns.len().to_string()), ("detail", &logic::brief_list(&clear_warns))],
                ));
            }
            match failed {
                None => {
                    self.workspace.open_mut().nav_stash.clear();
                    self.editor.session_mut().hist.saved_recipe = self.editor.doc().recipe.clone();
                    self.workspace.ui_mut().confirm_quit = false;
                    // The close guard re-checks on the way out. Everything
                    // above just persisted, so the re-check passes — but if
                    // anything is STILL dirty (logic drift, a future writer
                    // this loop misses), the bounce must be SAID: a silently
                    // re-armed dialog is the v0.21 dead-button livelock.
                    let residue = self.editor.inactive_dirty_variants(&self.workspace, );
                    if residue > 0 {
                        let t = trf(
                            lang,
                            K_ACTIONS_SAVED_BUT_VARIANT_S_STILL_COUNT_AS_UNSAVED_THE_W,
                            &[("n", &residue.to_string())],
                        );
                        self.workspace.toast(ToastKind::Error, t);
                    } else if !quit_warns.is_empty() {
                        // Everything IS saved (recipe-write-decides), so
                        // refusing to quit would be wrong — but a toast on a
                        // closing window dies unread. Bounce ONCE with the
                        // warnings visible; nothing is dirty any more, so the
                        // next quit passes straight through without this
                        // dialog. One extra click, only in the failure case.
                        let t = trf(
                            lang,
                            K_ACTIONS_SAVED_WITH_WARNINGS_THE_WINDOW_STAYS_OPEN_SO_THE,
                            &[("detail", &quit_warns.join(" · "))],
                        );
                        self.workspace.set_status(t.clone());
                        self.workspace.toast(ToastKind::Error, t);
                    } else {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                Some(e) => {
                    // Stay open: a failed save on quit must never quietly quit.
                    let t = trf(lang, K_ACTIONS_SAVE_FAILED, &[("err", &e)]);
                    self.workspace.set_status(t.clone());
                    self.workspace.toast(ToastKind::Error, t);
                    self.workspace.ui_mut().confirm_quit = false;
                }
            }
        } else if discard_quit {
            self.workspace.open_mut().nav_stash.clear();
            self.editor.session_mut().hist.saved_recipe = self.editor.doc().recipe.clone();
            // The answer to the guard's question, recorded: these two lines
            // clean the ACTIVE canvas, but a background variant's edits have
            // nowhere to be saved, so nothing can make them "clean" — only
            // the user's decision can end the question.
            self.workspace.open_mut().discard_requested = true;
            self.workspace.ui_mut().confirm_quit = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if cancel || !open {
            self.workspace.ui_mut().confirm_quit = false;
        }
    }

    /// Migrate every gallery photo's sidecars from a user-picked pre-store
    /// ./out folder (Settings button). Worker-side: one read_dir per photo
    /// over a possibly huge folder is I/O the UI thread must not pay.
    pub(crate) fn start_import_legacy(&mut self, dir: PathBuf) {
        if self.workspace.busy() {
            return;
        }
        let lang = self.lang;
        let photos = self.workspace.gal().gallery.clone();
        if photos.is_empty() {
            *self.workspace.status_mut() = tr(
                lang,
                K_ACTIONS_OPEN_A_FOLDER_FIRST_IMPORT_MIGRATES_THE_PHOTOS_C,
            )
            .into();
            return;
        }
        self.workspace.set_busy(true);
        *self.workspace.status_mut() = trf(
            lang,
            K_ACTIONS_IMPORTING_DEVELOPS_FROM,
            &[("path", &dir.display().to_string())],
        );
        self.services.spawn_worker(
            move || {
                // One directory scan for the whole gallery — the per-photo
                // migrate_legacy_from re-scanned the (possibly huge) legacy
                // folder once per photo, making import O(photos × entries).
                // FACTS out (L12#4): the landing renders the summary.
                let n = autoshade::store::migrate_legacy_from_many(&dir, &photos);
                Msg::LegacyImported(Ok((n, dir)))
            },
            |e| Msg::LegacyImported(Err(e)),
        );
    }

    /// Read the reference library's status onto a worker thread (R23-2).
    ///
    /// Off the UI thread because the file it parses can reach tens of MB, and
    /// CACHED in `style_info` because the panel that shows it draws every
    /// frame. Re-run after a build; nothing else invalidates it (another
    /// process rebuilding the shared library mid-session is the one staleness
    /// this accepts — the alternative is a stat per frame forever).
    pub(crate) fn start_style_info(&mut self) {
        if self.workspace.ai().style_info_loading {
            return;
        }
        self.workspace.ai_mut().style_info_loading = true;
        self.services.spawn_worker(
            || Msg::StyleInfo(Box::new(autoshade::library::summary())),
            // A read that PANICS must still clear the flag, or the status line
            // says "reading…" for the rest of the session.
            |e| Msg::StyleInfo(Box::new(autoshade::library::Summary::Unusable(e.to_string()))),
        );
    }

    /// Build (or rebuild) the reference library from a folder of finished
    /// photos (R23-2, feedback #6: the desktop app's own entry point).
    ///
    /// Background, with progress, and NOT `busy`-gated — freezing the whole
    /// app for minutes would be its own defect — so only this button gates on
    /// `style_build_inflight`. No cancel: the build has no cancellation
    /// checkpoints, and closing the window mid-build is safe because the
    /// library is published atomically (`Library::save`), so a killed process
    /// leaves the previous library intact.
    pub(crate) fn start_style_build(&mut self, dir: PathBuf) {
        if self.workspace.ai().style_build_inflight {
            return;
        }
        self.workspace.ai_mut().style_build_inflight = true;
        self.workspace.ai_mut().style_build_progress = None;
        *self.workspace.status_mut() = trf(
            self.lang,
            K_ACTIONS_BUILDING_THE_REFERENCE_LIBRARY_FROM_THE_FIRST_BU,
            &[("path", &abs_display(&dir))],
        );
        // The progress channel is the app's own sender: `spawn_worker` delivers
        // ONE terminal message, and this build has to report while it runs.
        let tx = self.services.tx.clone();
        let ctx = self.services.egui_ctx.clone();
        // Resolved HERE, on the UI thread: a preference flipped mid-build must
        // not change what this build is doing.
        let describe = autoshade::library::DescribeSwitch::resolve(None, self.aip.style_describe);
        let raw_root = self.aip.style_raw_dir.clone().filter(|d| d.is_dir());
        self.services.spawn_worker(
            move || {
                let progress = |p: autoshade::library::BuildProgress| {
                    let _ = tx.send(Msg::StyleBuildProgress { stage: p.stage, done: p.done, total: p.total });
                    // An mpsc send does not wake egui (see `spawn_worker`).
                    ctx.request_repaint();
                };
                let sidecars = autoshade::library::Sidecars::from_config(&autoshade::config::Config::load());
                match autoshade::library::rebuild(&dir, &sidecars, describe, raw_root.as_deref(), &progress) {
                    Ok(lib) => Msg::StyleBuilt(Box::new(StyleBuildOutcome::Saved {
                        total: lib.entries.len(),
                        phrased: lib.entries.iter().filter(|e| e.phrases.is_some()).count(),
                        dir,
                    })),
                    Err(e) => Msg::StyleBuilt(Box::new(StyleBuildOutcome::Failed { err: format!("{e:#}") })),
                }
            },
            |e| Msg::StyleBuilt(Box::new(StyleBuildOutcome::Failed { err: e.to_string() })),
        );
    }

    /// The analyze worker every analysis starter shares — 「AI Analyze」,
    /// 「AI adjust」 and Normal's one-click looks (`panels/normal.rs`): arms
    /// the cancel, marks the wire busy and runs `produce_recipe` on a worker
    /// thread inside `library`'s scope (`library::with_library`), landing as
    /// `Msg::Analyzed` under the returned epoch. The caller has already set
    /// `busy` and its status line.
    pub(crate) fn spawn_analyze(
        &mut self,
        path: PathBuf,
        pixels: Option<PathBuf>,
        guidance: Option<String>,
        base: Option<EditRecipe>,
        req: autoshade::pipeline::GradeRequest,
        library: &'static str,
    ) -> u64 {
        #[cfg(test)]
        {
            self.workspace.normal_mut().last_request = Some((req, guidance.clone(), library));
        }
        #[cfg(test)]
        let runner = self.workspace.normal().runner;
        // Every other network starter arms this; Analyze — the one a stalled
        // endpoint holds the longest — did not, so a hung stream left no ✕
        // and only killing the process recovered. Abandon semantics (like the
        // local-compute starters): the advisor has no cancel checkpoints, so
        // ✕ frees the UI immediately, the call dies at its stall/progress
        // deadline, and the stale-epoch result is discarded on arrival.
        let (epoch, _flag) = self.workspace.arm_cancel();
        self.workspace.tools_mut().analyze_inflight = true;
        self.services.spawn_worker(
            move || {
                // Test seam: a headless test stands in for the paid chain.
                #[cfg(test)]
                if let Some(run) = runner {
                    return Msg::Analyzed(epoch, Box::new(run(&path, &req)));
                }
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&path))); // full-frame commit budget (budget.rs)
                // Config is reloaded in-thread (cheap) so we don't need it to be Clone.
                let cfg = autoshade::config::Config::load();
                let res = autoshade::library::with_library(library, || {
                    autoshade::pipeline::produce_recipe(
                        &path,
                        pixels.as_deref(),
                        &cfg,
                        false,
                        guidance.as_deref(),
                        base.as_ref(),
                        req,
                        // Interactive single-photo analyze: the visual closed
                        // loop IS the R20 strengthening (cost disclosed in the
                        // button's hover text; batch surfaces pass false).
                        true,
                        autoshade::diag::stderr(),
                    )
                });
                Msg::Analyzed(epoch, Box::new(res))
            },
            move |e| Msg::Analyzed(epoch, Box::new(Err(e))),
        );
        epoch
    }

    /// Reverse-fit ("match"): statistically solve the develop parameters that
    /// map the SOURCE neutral onto a finished rendition of this frame — the
    /// active「AI 生成」variant, or (R23-6) any reference file the user chose.
    /// The result lands as a new「反推」variant (base = source neutral, look in
    /// the recipe), and for a RAW the XMP sidecar is written immediately. The
    /// fit itself is deterministic and local; only the optional review calls
    /// out.
    ///
    /// The base is `source_preview`, NOT `base_preview`: after a reimagine the
    /// active variant's base IS the generated raster, and fitting a rendition
    /// onto itself would recover ~neutral. Fit must map the negative → the look.
    pub(crate) fn start_fit(&mut self) {
        let (Some(base), Some(tgt)) = (self.editor.photo().source_preview.clone(), self.editor.fit_target(&self.workspace))
        else {
            return;
        };
        if self.workspace.busy() {
            return;
        }
        let src_path = self.editor.photo().src_path.clone();
        // The source the fold names (`fit_source_index`: the user's pick, else
        // the automatic negative): the source frame below, the ◭ card's origin
        // and the persisted pixel link all read this ONE capture, so the three
        // cannot disagree.
        let negative = self.fit_negative();
        let fit_strength = self.aip.fit_strength();
        let zoned = self.workspace.fit().zoned_fit;
        let zoned_regions = if self.workspace.fit().zoned_four_regions {
            autoshade::fit_zoned::semantic::MAX_SEMANTIC_REGIONS
        } else {
            autoshade::fit_zoned::semantic::DEFAULT_SEMANTIC_REGIONS
        };
        let ai_judge = self.workspace.fit().fit_ai_judge;
        // The deep path IS the review, iterated — it cannot run without it,
        // and the checkbox is disabled accordingly, but the worker must not
        // depend on a UI gate for a spending decision.
        let deep = self.workspace.fit().fit_deep && ai_judge;
        let lang = self.lang;
        self.workspace.set_busy(true);
        let mut running = if zoned {
            tr(lang, K_ACTIONS_REVERSE_FITTING_GLOBAL_SEMANTIC_RANGES_SPATIAL_T).to_string()
        } else {
            tr(lang, K_ACTIONS_REVERSE_FITTING_STATISTICAL_FIT_LOCAL_COMPUTE).to_string()
        };
        if deep {
            running.push_str(tr(lang, K_ACTIONS_RETRY_AI_REVIEW_BEFORE_SAVING_UP_TO_ONE_GUIDED_R));
        } else if ai_judge {
            // The review runs AFTER the local fit and is a network call — the
            // busy line must own that extra wait up front.
            running.push_str(tr(lang, K_ACTIONS_THEN_AI_REVIEW_VISION_CALL));
        }
        self.workspace.set_status(running);
        self.services.spawn_worker(
            move || {
                // Full-frame commit budget (budget.rs); the reference's own
                // header raises the floor when it is the bigger file.
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&tgt)));
                let res = (|| -> anyhow::Result<FitOutcome> {
                    // THE raw-vs-baked dispatch (R22-1): the reference may now
                    // be any file the user picked, including a RAW, which
                    // `decode::load_image` refuses by name. `source_pixels`
                    // develops a RAW neutrally and loads a baked file — the
                    // one branch, never hand-copied. The cap is comfortably
                    // above both consumers (the fit analyses at 384, the judge
                    // at 1024) and preserves aspect, which the same-frame
                    // check reads.
                    const FIT_REF_EDGE: u32 = 2048;
                    let target = autoshade::render::source_pixels(&tgt, Some(FIT_REF_EDGE))?;
                    // The gate runs at PERSIST time (below), not up front:
                    // a pre-fit snapshot left the whole multi-minute
                    // segmentation as a race window — an explicit save
                    // landing during the fit was then overwritten
                    // unversioned. (Claimed unique rasters already made the
                    // fit itself harmless to the saved develop.)
                    // Automatic local pass only when enabled AND the photo
                    // has a real path (the semantic raster needs a home).
                    // Segmentation success produces semantic sky/land; an
                    // unavailable sidecar falls through to native luminance
                    // ranges inside fit_recipe_zoned_with_regions. The raster
                    // gets a FRESH unique name per fit (mask-zone-sky.png,
                    // -2, -3, …, create_new-claimed like every master name):
                    // the old fixed name was rewritten IN PLACE before the
                    // recipe landed, so a crash or a failed recipe write left
                    // the still-live saved recipe pointing at the NEW bytes —
                    // the same shared-file corruption class that once made
                    // the fit's cleanup delete 「AI select sky」's raster.
                    // With unique names the saved develop's raster is never
                    // touched, so the pass no longer needs the backup gate;
                    // superseded rasters stay on disk (tiny greyscale PNGs —
                    // v<N> snapshots freeze their own copies regardless).
                    // R15/R16: the photo's calibration composes INTO the
                    // solve — the recipe starts from the calibration-only
                    // base, every closed-loop candidate render is the
                    // canvas's own one-pass user(base(x)), and the
                    // deliverable carries the calibration by construction
                    // (no separate stamp, no two-pass seed render). Fitting
                    // from the raw neutral burned the bounded model on
                    // re-deriving the 0.6–1.4 EV camera look (real pair:
                    // saturation pegged, cast curves vetoed, a helpful sky
                    // zone dropped); the v0.24.0 pre-rendered seed then
                    // clipped saturated channels at the pass boundary —
                    // both retired by pipeline::calibration_recipe +
                    // fit_recipe_from.
                    // …and since R33 §A the SOURCE FRAME comes from the same
                    // one function the CLI uses: a 2048-px neutral develop of
                    // the sensor frame. The panel's 1280-px `source_preview`
                    // is a different image of the same capture — different
                    // resampling, different sharpening — and the structural
                    // reading that picks Full vs Atmosphere is measured on
                    // whichever image the entry chose. A non-RAW source has no
                    // sensor frame, so it keeps the preview it already has.
                    // …and when the ▣ card hangs off an in-place master
                    // (2026-09-13), THAT is the source frame: a neutral
                    // develop already, loaded as-is through the same dispatch
                    // (a full-res master is capped to the fit's working edge
                    // by the same area average the RAW arm downscales with),
                    // with the photo's calibration composing on top exactly
                    // as the ▣ card renders it — the master carries none.
                    // The fit source is a `render::Plate` held while this fit
                    // runs (`plate`): a RAW's keeps its camera frame, so the
                    // candidates that move the white are rendered with it in
                    // the profile stage, and every plate says which frame its
                    // pixels are in (a master's marker, v1.17.0).
                    let (mut plate, fit_base, raw_source) = match (src_path.as_deref(), negative.as_deref()) {
                        (Some(p), Some(master)) => {
                            let cal = autoshade::pipeline::calibration_recipe(
                                autoshade::pipeline::fit_calibration(p),
                            );
                            let pixels = autoshade::render::source_pixels(
                                master,
                                Some(autoshade::pipeline::FIT_SOURCE_EDGE),
                            )?;
                            (autoshade::pipeline::fit_source_baked(master, pixels)?, cal, None)
                        }
                        (Some(p), None) if autoshade::decode::is_raw(p) => {
                            let (frame, cal) = autoshade::pipeline::fit_source(p)?;
                            (frame, cal, Some(p))
                        }
                        (Some(p), _) => {
                            let cal = autoshade::pipeline::calibration_recipe(
                                autoshade::pipeline::fit_calibration(p),
                            );
                            (autoshade::pipeline::fit_source_baked(p, (*base).clone())?, cal, None)
                        }
                        (None, _) => (autoshade::render::Plate::baked((*base).clone()), Default::default(), None),
                    };
                    // The source in the target's frame — the CLI's rule
                    // (`pipeline::fit_target`). The target is paired and
                    // judged as it was exported.
                    let mut fit_base = fit_base;
                    let fitted_target =
                        autoshade::pipeline::fit_target(&mut plate, raw_source, &target, &mut fit_base)?;
                    let base = plate.shared();
                    let fit_target = &fitted_target.image;
                    // ONE judge call site for both orderings (R23-6): the
                    // default path reviews AFTER the persist, the deep path
                    // reviews BEFORE it and again after its retry, and a
                    // second hand-written copy of "encode both frames at
                    // detail:high tile size and ask FitMatch" is exactly how
                    // the two would drift into judging different pixels.
                    // Domain: `rep.recipe` renders from the source NEUTRAL
                    // (`base` — the room the composed fit solved in), so that
                    // render against the target is the pair the fit's own
                    // residual measures.
                    const JUDGE_EDGE: u32 = 1024; // detail:high tiles at 512 px — 4 tiles read a grade
                    let judge_of = |recipe: &autoshade::recipe::EditRecipe|
                     -> anyhow::Result<autoshade::advisor::Judgement> {
                        let enc = |img: &image::DynamicImage| -> anyhow::Result<Vec<u8>> {
                            let mut j = Vec::new();
                            img.write_to(
                                &mut std::io::Cursor::new(&mut j),
                                image::ImageFormat::Jpeg,
                            )?;
                            Ok(j)
                        };
                        let fitted = autoshade::render::develop_preview(
                            &autoshade::render::Plate::thumbnail_of(&base, JUDGE_EDGE),
                            recipe,
                        );
                        let t_jpeg = enc(&autoshade::render::thumbnail(&target, JUDGE_EDGE, JUDGE_EDGE))?;
                        let f_jpeg = enc(&fitted)?;
                        Ok(autoshade::advisor::judge_pair(
                            autoshade::advisor::JudgeImages {
                                reference: &t_jpeg,
                                candidate: &f_jpeg,
                            },
                            autoshade::advisor::JudgeTask::FitMatch,
                            None,
                            // FitMatch scores a MATCH between two renders —
                            // the strength axis has no bearing on it (R23-3).
                            None,
                        )?)
                    };
                    // Returns the raster this run CLAIMED alongside the report:
                    // `claim_raster` hands out a fresh `mask-zone-sky[-N].png`
                    // and only the recipe that keeps the solve ever references
                    // it, so a discarded candidate would leave a zero-reference
                    // file behind (R23 review LOW-5). The claimed PATH, never a
                    // rebuilt name — the claim may be the `-2` / `-3` suffix.
                    // 完全自动 (user ruling 2026-08-26): the fit's D gate may
                    // consult the local DIFT sidecar on content-divergent
                    // pairs; failures degrade into the rationale.
                    let corr = autoshade::correspond::fit_provider(
                        autoshade::correspond::CorrespondOpts::from_config(
                            &autoshade::config::Config::load(),
                        ),
                    );
                    let run_zoned = |seg_on: bool|
                     -> anyhow::Result<(autoshade::fit::FitReport, Option<std::path::PathBuf>)> {
                        Ok(match (seg_on, &src_path) {
                            (true, Some(p)) => {
                                let cfg = autoshade::config::Config::load();
                                let seg =
                                    autoshade::segment::SegmentOpts::from_config(&cfg, "sky");
                                // Drawn in the source's frame, and marked so.
                                let mask = autoshade::store::OwnedRaster::claim(p, "mask-zone-sky")?
                                    .with_frame(autoshade::render::fit_raster_frame(&base, &fit_base));
                                let rep = autoshade::fit_zoned::fit_recipe_zoned_with_regions(
                                    &base,
                                    fit_target,
                                    &seg,
                                    &mask,
                                    &fit_base,
                                    autoshade::fit::FitOptions {
                                        strength: fit_strength,
                                        provider: Some(&corr),
                                    },
                                    zoned_regions,
                                );
                                (rep, Some(mask.into_path()))
                            }
                            _ => (
                                autoshade::fit::fit_recipe_from_with(
                                    &base,
                                    fit_target,
                                    &fit_base,
                                    autoshade::fit::FitOptions {
                                        strength: fit_strength,
                                        provider: Some(&corr),
                                    },
                                ),
                                None,
                            ),
                        })
                    };
                    // The FIRST solve's claim is deliberately not swept: this
                    // report is the one that ships unless a candidate beats it,
                    // and a candidate can only be the zoned pass when this solve
                    // was NOT zoned (`hint_action`'s `zoned_used` fact), i.e.
                    // when it claimed nothing at all.
                    let (mut rep, _first_claim) = run_zoned(zoned)?;
                    autoshade::pipeline::note_target_geometry(&mut rep, fitted_target.geometry);
                    // FACTS, not prose (L12#4): the landing renders these
                    // with the language live when the result LANDS — the old
                    // trf calls here used the language captured at spawn,
                    // minutes stale after a segmentation run.
                    let mut status: Vec<FitNote> = Vec::new();
                    // FIRST, before every other landing fact: which solver ran
                    // and at which pairing scale. Everything else on this line
                    // is a consequence of those two.
                    let reading = |r: Option<autoshade::fit::Divergence>| {
                        r.map_or_else(|| "unmeasured".to_string(), |r| format!("{:.3}", r.d))
                    };
                    status.push(FitNote::SolverMode {
                        atmosphere: rep.mode == autoshade::fit::FitMode::Atmosphere,
                    });
                    status.push(FitNote::SolverPairing {
                        pixel: rep.pairing == autoshade::fit::PairingScale::Pixel,
                        fine: reading(rep.divergence),
                        coarse: reading(rep.divergence_coarse),
                    });
                    // ── the DEEP path (R23-6 D). Everything here happens
                    // BEFORE the persist below, which is the whole point and
                    // the deliberate revision of R20's ordering — recorded at
                    // the persist site.
                    //
                    // Discipline, copied wholesale from `visual_review_round`
                    // (pipeline.rs) because it is the shape that already
                    // survived review: the retry must re-score AT LEAST as
                    // high to be kept, an action that changes nothing
                    // short-circuits before spending a second call, and every
                    // failure keeps the plain solve and degrades to a note.
                    //
                    // `judged` is THREE-state, the same shape the CLI's
                    // `--deep` path uses: `None` = the deep path never ran,
                    // `Some(Ok(_))` = a verdict describing what ships,
                    // `Some(Err(_))` = the deep review ran and FAILED. The last
                    // one used to be indistinguishable from "never ran", so the
                    // informational block below bought a THIRD network attempt
                    // after two had already failed — past the two-paid-calls
                    // ceiling its own tooltip states (R23 review LOW-6).
                    let mut judged: Option<anyhow::Result<autoshade::advisor::Judgement>> = None;
                    if deep {
                        match judge_of(&rep.recipe) {
                            Ok(first) => {
                                let action = hint_action(
                                    first.hint.as_deref().unwrap_or(""),
                                    !rep.recipe.masks.is_empty(),
                                    !zoned && src_path.is_some(),
                                );
                                let candidate = match action {
                                    // The one action that needs the solver
                                    // again. A claim/segmentation failure is
                                    // not an error here: the plain solve in
                                    // hand is already a valid result.
                                    FitAction::Zoned => run_zoned(true).ok().map(|(mut r, claim)| {
                                        autoshade::pipeline::note_target_geometry(&mut r, fitted_target.geometry);
                                        (r, claim)
                                    }),
                                    FitAction::Saturation(d) => {
                                        let mut r = rep.recipe.clone();
                                        r.saturation += d;
                                        r.clamp();
                                        // Identical settings ⇒ nothing to
                                        // judge, and no second call bought.
                                        (r.saturation != rep.recipe.saturation).then(|| {
                                            // RE-DERIVED, never cloned off the
                                            // solve (R23 review MED-3): every
                                            // outcome note — the residual pair,
                                            // the joint numbers, the
                                            // unrepresented-controls diagnosis,
                                            // the terminal-reset verdict the
                                            // FitReset check below reads — is a
                                            // statement about the recipe, and
                                            // this recipe is not the one the
                                            // solver reported on.
                                            (
                                                autoshade::fit::rescore_report(
                                                    &base,
                                                    fit_target,
                                                    &r,
                                                    &fit_base,
                                                    rep.err_before,
                                                    &rep.notes,
                                                ),
                                                None,
                                            )
                                        })
                                    }
                                    FitAction::None => None,
                                };
                                let outcome = match candidate {
                                    Some((mut cand, claimed)) => {
                                        let verdict = judge_of(&cand.recipe);
                                        let keep = matches!(
                                            &verdict,
                                            Ok(second) if second.score >= first.score
                                        );
                                        if !keep {
                                            // The candidate is gone, so its
                                            // claimed raster is now referenced
                                            // by nothing (R23 review LOW-5).
                                            // ONLY this run's claim is touched,
                                            // by the path `claim_raster`
                                            // returned. Best effort by design:
                                            // a failure leaves an inert
                                            // greyscale PNG the user can delete,
                                            // and reporting it would be noise on
                                            // a fit that succeeded.
                                            if let Some(p) = &claimed {
                                                let _ = std::fs::remove_file(p);
                                            }
                                        }
                                        match verdict {
                                            Ok(second) if keep => {
                                                autoshade::rationale::push_note(
                                                    &mut cand.recipe.rationale,
                                                    &mut cand.notes,
                                                    autoshade::rationale::Note::new(
                                                        autoshade::rationale::keys::FIT_NOTE_DEEP_ADOPTED,
                                                        vec![
                                                            ("score1", format!("{:.0}", first.score)),
                                                            ("score2", format!("{:.0}", second.score)),
                                                            ("action", action.tag().to_string()),
                                                        ],
                                                    ),
                                                );
                                                rep = cand;
                                                judged = Some(Ok(second));
                                                DeepFitOutcome::Adopted
                                            }
                                            // Lower, or un-judgeable: the
                                            // plain solve stands, and the
                                            // FIRST verdict is the one that
                                            // describes what ships.
                                            Ok(_) => {
                                                judged = Some(Ok(first));
                                                DeepFitOutcome::Discarded
                                            }
                                            Err(e) => {
                                                status.push(FitNote::AiReviewFailed(e.to_string()));
                                                judged = Some(Ok(first));
                                                DeepFitOutcome::Discarded
                                            }
                                        }
                                    }
                                    // No candidate, and WHY differs (R23
                                    // review LOW-4): `FitAction::None` is the
                                    // reviewer having named nothing this app
                                    // can do, while a selected action that
                                    // produced no candidate is the app failing
                                    // to carry out a move it did choose — the
                                    // zoned re-solve errored, or the saturation
                                    // step clamped back to the value in hand.
                                    None => {
                                        judged = Some(Ok(first));
                                        match action {
                                            FitAction::None => DeepFitOutcome::NothingActionable,
                                            _ => DeepFitOutcome::ActionDidNotRun,
                                        }
                                    }
                                };
                                status.push(FitNote::DeepFit { action: action.tag(), outcome });
                            }
                            Err(e) => {
                                status.push(FitNote::AiReviewFailed(e.to_string()));
                                judged = Some(Err(e));
                            }
                        }
                    }
                    status.extend(fit_mask_notes(&rep.recipe, &rep.notes));
                    // R23-6 A-3: the terminal do-no-harm reset is "the
                    // reverse-fit did nothing", and a line inside a rationale
                    // block is not where a user finds that out.
                    if rep.notes.iter().any(|n| n.key == autoshade::rationale::keys::FIT_NOTE_REGRESSED)
                    {
                        status.push(FitNote::FitReset);
                    }
                    if rep
                        .notes
                        .iter()
                        .any(|n| n.key == autoshade::rationale::keys::FIT_NOTE_NOT_SAME_FRAME)
                    {
                        status.push(FitNote::ReferenceNotSameFrame);
                    }
                    let mut persisted = false;
                    if let Some(p) = &src_path {
                        // Worker thread ⇒ Wait, ONE lock across gate →
                        // recipe → pixel-link clear → XMP: still the
                        // narrowest gate window, now with no other process
                        // interleaving between the halves. The
                        // identical-content skip keeps the gate from
                        // spamming versions on repeated fits.
                        let persist = autoshade::store::with_develop_lock(
                            p,
                            autoshade::store::DevelopLockMode::Wait,
                            || -> std::io::Result<()> {
                        let backed = autoshade::store::backup_saved_develop(p, Some(&rep.recipe));
                        // Persist the fit losslessly: recipe.json carries the
                        // bitmap zone masks + exact recolour gains the XMP cannot,
                        // so reopening the photo restores the whole fit. The
                        // snapshot above is the gate — if it FAILED, skip the
                        // persist entirely (the fit stays on the canvas with
                        // ● unsaved; Ctrl+S overwrites explicitly).
                        match &backed {
                            Ok(backed) => {
                              // ONE single-generation commit (the Ctrl+S
                              // rule, L03): the fit's recipe and the
                              // pixel link land together. The fit is a
                              // SOURCE-based develop by construction (it maps
                              // the source neutral onto the rendition), and a
                              // stale pixels.json link surviving the recipe
                              // write made a reopen render the fit on baked
                              // pixels it was never computed against — so the
                              // link is CLEARED, unless the negative itself
                              // hangs off an in-place master (2026-09-13):
                              // then the fit WAS computed against those
                              // pixels, the ◭ card lands on them, and the
                              // link records them as the ▣ card's own Ctrl+S
                              // would (`inplace`, never `generated`).
                              // The projection rides the same generation
                              // (2026-09-13): a fit is a source develop, so
                              // it always projects; a projection this build
                              // cannot produce degrades to Keep + the
                              // XmpFailed note, the recipe still landing.
                              let (xmp_member, xmp_outcome) =
                                  match autoshade::pipeline::xmp_projection_member(
                                      p,
                                      &rep.recipe,
                                      true,
                                      autoshade::diag::stderr(),
                                  ) {
                                      Ok((m, note, _)) => (m, Ok(note)),
                                      Err(e) => (autoshade::store::CommitMember::Keep, Err(e)),
                                  };
                              let commit_res: anyhow::Result<()> = (|| {
                                  autoshade::store::commit_develop(
                                      p,
                                      autoshade::store::DevelopCommit {
                                          recipe: Some(autoshade::pipeline::recipe_store_bytes(
                                              p,
                                              &rep.recipe,
                                              autoshade::diag::stderr(),
                                          )?),
                                          pixels: match negative.as_deref() {
                                              Some(m) => autoshade::store::CommitMember::Write(
                                                  autoshade::store::pixel_source_record_bytes(
                                                      p, m, false,
                                                  )?,
                                              ),
                                              None => autoshade::store::CommitMember::Clear,
                                          },
                                          // R24-4: this worker publishes a
                                          // REVERSE-FIT into the active slot,
                                          // and the strip record's active
                                          // half has to say so — left `Keep`,
                                          // a photo whose record read
                                          // 「original」 reopened as 「▣ 原片」
                                          // holding the fit. Worker thread:
                                          // no live strip to hand over, so
                                          // the shared primitive restates the
                                          // one fact this writer owns (the
                                          // card's id/name/position stand).
                                          variants: autoshade::store::variants_member(
                                              p,
                                              autoshade::store::ActiveWrite::Kind("fitted"),
                                          )?,
                                          xmp: xmp_member,
                                      },
                                  )?;
                                  Ok(())
                              })();
                              match commit_res {
                              Err(e) => {
                                // A failed store write must NOT discard the
                                // minutes of segmentation + fitting that
                                // already succeeded — same degrade shape as
                                // the backup-gate branch below: the fit lands
                                // on the canvas unsaved.
                                status.push(FitNote::NotPersistedCommit(e.to_string()));
                              }
                              Ok(()) => {
                                persisted = true;
                                if autoshade::decode::is_raw(p) {
                                    // The recipe write ALONE decides the saved
                                    // state (same rule as Ctrl+S / Analyze):
                                    // an XMP failure must not collapse an
                                    // already-persisted fit into an error —
                                    // reopening WOULD restore it, so the UI
                                    // must agree that it saved.
                                    // The M6a list is dropped HERE: the zoned
                                    // fit's own rationale already states that
                                    // its corrections are bitmap masks the
                                    // Lightroom sidecar cannot carry (the
                                    // ZONE_ATTACHED note), so a second copy
                                    // would say it twice for the same masks.
                                    match xmp_outcome {
                                        Ok(merge_note) => {
                                            status.push(FitNote::XmpWritten(
                                                autoshade::pipeline::xmp_target(p),
                                            ));
                                            // Regenerated-not-merged: same
                                            // disclosure as Ctrl+S.
                                            if let Some(m) = merge_note {
                                                status.push(FitNote::XmpMergeNote(m));
                                            }
                                        }
                                        Err(e) => {
                                            status.push(FitNote::XmpFailed(e.to_string()));
                                        }
                                    }
                                }
                                if let Some(n) = backed {
                                    status.push(FitNote::BackedUpAs(*n));
                                }
                              }
                              }
                            }
                            Err(e) => {
                                status.push(FitNote::NotPersistedBackup(e.to_string()));
                            }
                        }
                                Ok(())
                            },
                        );
                        if let Err(e) = persist {
                            status.push(FitNote::NotPersistedLock(e.to_string()));
                        }
                    }
                    // Opt-in AI review (LLM-as-a-judge): the vision model
                    // scores how faithfully the fitted render matches the
                    // target look.
                    //
                    // ORDERING, and its history. R20 decided this review runs
                    // AFTER the persist on purpose — informational, never
                    // gating, delaying or failing the fit's landing, every
                    // failure degrading to a status note (docs/ROADMAP.md's
                    // R20 entry; Opus S5 定案). That is still the DEFAULT
                    // path below, unchanged to the byte.
                    //
                    // R23-6 revises it for one explicitly-chosen case: with
                    // 「deep」 ticked the user has asked the reviewer to act,
                    // which it cannot do after the recipe is on disk. The deep
                    // block above therefore reviews BEFORE the persist and may
                    // buy ONE bounded guided retry (user decision 2026-08-17
                    // ⑥). The R20 rule is not repealed — it is now the
                    // behaviour of the unticked box, which is also the
                    // default, and the historical entry stays as written.
                    //
                    // Domain: rep.recipe renders from the source NEUTRAL
                    // (source_preview — the room the composed fit solved in),
                    // so that render vs the target is exactly the pair the
                    // fit's own residual measures.
                    if ai_judge {
                        // The two-paid-calls ceiling, decided by a pure
                        // function so it is a pinned property and not a shape
                        // one has to re-read this closure to check
                        // (R23 review LOW-6).
                        let verdict = match FitReviewPlan::of(&judged) {
                            FitReviewPlan::Reuse => judged,
                            FitReviewPlan::Skip => None,
                            FitReviewPlan::Call => Some(judge_of(&rep.recipe)),
                        };
                        match verdict {
                            Some(Ok(j)) => status.push(FitNote::AiReview {
                                score: j.score,
                                critique: j.critique,
                                // R23-6: SHOWN, never executed on this path.
                                // R20 dropped it silently and the user paid
                                // for it.
                                hint: j.hint,
                            }),
                            Some(Err(e)) => status.push(FitNote::AiReviewFailed(e.to_string())),
                            None => {}
                        }
                    }
                    Ok(FitOutcome {
                        err_before: rep.err_before,
                        err_after: rep.err_after,
                        rationale_notes: rep.notes,
                        recipe: rep.recipe,
                        status,
                        persisted,
                        negative,
                    })
                })();
                Msg::Fitted(Box::new(res))
            },
            |e| Msg::Fitted(Box::new(Err(e))),
        );
    }
}
