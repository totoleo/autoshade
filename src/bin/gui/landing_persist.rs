//! The analysis landing's PERSISTENCE: the backup gate, one develop commit
//! (recipe + pixel link + strip + XMP member in a single generation) and the
//! sidecar disclosures. The landing itself lives in `workers::on_analyzed`;
//! this file is the write half, with its own status answer.
//!
//! The lock is taken NO-WAIT: busy must degrade to "applied but NOT saved",
//! never hang the UI thread behind another process.

use crate::*;

impl Commands<'_> {
    /// Persist one accepted analysis result. `stamped` is the recipe the store
    /// writes (the RAW's own develop — the canvas copy may differ on a baked
    /// card); `p` is the open photo.
    pub(crate) fn persist_analysis_landing(&mut self, lang: Lang, p: PathBuf, stamped: EditRecipe) -> String {
match autoshade::store::with_develop_lock(
    &p,
    autoshade::store::DevelopLockMode::NoWait,
    || -> std::io::Result<String> {
        Ok(match autoshade::store::backup_saved_develop(&p, Some(&stamped)) {
    Err(e) => self.workspace.show_words(
        lang,
        logic::analysis_landing::words(
            &logic::analysis_landing::SaveReport::BackupFailed {
                err: e.to_string(),
            },
        ),
    ),
    Ok(backed) => {
        // The recipe write ALONE decides the saved
        // state (same rule as save_xmp): once it
        // lands, reopening restores it regardless
        // of the XMP — so the ● baseline follows
        // it even when the XMP half fails.
        // Analyze is a SAVER (same rule as
        // Ctrl+S) — ONE single-generation
        // commit (L03): recipe + pixel link
        // land whole or not at all, so the
        // stash protection stays armed on any
        // failure instead of half a save
        // declaring everything safe.
        // The STRIP lands in the same generation
        // (R24-4): Analyze used to commit
        // `variants: Keep`, so a record written
        // before the user switched cards kept
        // describing the OLD active card while
        // recipe.json already held this one's AI
        // develop — the registered cross-surface
        // residual, closed here. This landing
        // runs on the UI thread and therefore
        // OWNS the live strip: it hands the whole
        // record over (`ActiveWrite::Strip`),
        // exactly like Ctrl+S, and advances the
        // mirror below on success.
        let origin = self.editor.active_variant()
            .and_then(|v| v.origin.clone());
        let generated = self.editor.active_on_ai_pixels();
        let strip_rec = self.editor.current_strip_record();
        // The projection is a member of the same
        // generation (2026-09-13) — and for a
        // GENERATED canvas it CLEARS: this landing
        // used to write a sidecar claiming the
        // RAW carried the analysis grade that was
        // in fact tuned over AI pixels.
        let (xmp_member, xmp_outcome) =
            match autoshade::pipeline::xmp_projection_member(
                &p,
                &stamped,
                !generated,
                autoshade::diag::stderr(),
            ) {
                Ok((m, note, losses)) => (m, Ok((note, losses))),
                Err(e) => (autoshade::store::CommitMember::Keep, Err(e)),
            };
        let commit_res: anyhow::Result<()> = (|| {
            let recipe_bytes =
                autoshade::pipeline::recipe_store_bytes(&p, &stamped, autoshade::diag::stderr())?;
            let pixels = match &origin {
                Some(o) => autoshade::store::CommitMember::Write(
                    autoshade::store::pixel_source_record_bytes(
                        &p, o, generated,
                    )?,
                ),
                None => autoshade::store::CommitMember::Clear,
            };
            let variants = autoshade::store::variants_member(
                &p,
                autoshade::store::ActiveWrite::Strip(
                    strip_rec.as_ref(),
                ),
            )?;
            autoshade::store::commit_develop(
                &p,
                autoshade::store::DevelopCommit {
                    recipe: Some(recipe_bytes),
                    pixels,
                    variants,
                    xmp: xmp_member,
                },
            )?;
            Ok(())
        })();
        match commit_res {
            Ok(()) => {
                self.workspace.gal_mut().edited_badge.clear();
                // The ● baseline lives in CANVAS
                // coordinates: on a baked variant
                // the canvas copy dropped the
                // curve + lens profile, and a
                // baseline keeping them (the
                // disk form) lit ● the instant a
                // successful analyze landed.
                self.editor.session_mut().hist.saved_recipe = self.editor.doc().recipe.clone();
                self.workspace.open_mut().nav_stash.remove(&p);
                self.editor.session_mut().hist.pixels_on_disk = origin;
                // The strip mirror advances WITH
                // the commit (the Ctrl+S rule):
                // all-or-nothing, so a failed
                // commit leaves it untouched and
                // the background-variant unsaved
                // protection armed.
                self.editor.session_mut().hist.saved_strip = strip_rec;
                self.editor.forget_open_base(self.workspace);
                if backed.is_some() {
                    self.refresh_versions();
                }
                let mut s = self.workspace.show_words(
                    lang,
                    logic::analysis_landing::words(
                        &logic::analysis_landing::SaveReport::Saved { backed_up: backed },
                    ),
                );
                if autoshade::decode::is_raw(&p) && !generated {
                    match xmp_outcome {
                        // Regenerated-not-merged AND
                        // the M6a projection losses:
                        // the same disclosure as
                        // Ctrl+S, in the same two
                        // voices. Analyze is where
                        // AI bitmap masks arrive, so
                        // this landing needs the
                        // export-side line as much
                        // as the save does.
                        Ok((merge_note, losses)) => {
                            // A regenerated-not-merged
                            // sidecar always
                            // interrupts: it dropped
                            // the user's own
                            // Lightroom properties.
                            if let Some(m) = merge_note {
                                self.workspace.toast(ToastKind::Error, m.clone());
                                s = format!("{s} — ⚠ {m}");
                            }
                            // The projection's own
                            // losses, with Ctrl+S's
                            // interrupt rule (MED-2 —
                            // engine calibration is
                            // said quietly, a user's
                            // own mask is not).
                            let globals =
                                autoshade::xmp::global_export_losses(
                                    &stamped,
                                );
                            if let Some(m) =
                                xmp_loss_line(lang, &losses, &globals)
                            {
                                if xmp_loss_interrupts(&losses, &globals)
                                {
                                    self.workspace.toast(
                                        ToastKind::Error,
                                        m.clone(),
                                    );
                                    s = format!("{s} — ⚠ {m}");
                                } else {
                                    s = format!("{s} · {m}");
                                }
                            }
                            // R25 B4, the other
                            // corner: what LR
                            // renders and this
                            // canvas does not.
                            // Quiet channel only —
                            // nothing was lost.
                            if let Some(m) = render_gap_line(
                                lang,
                                &autoshade::xmp::global_render_gaps(
                                    &stamped,
                                ),
                            ) {
                                s = format!("{s} · {m}");
                            }
                        }
                        Err(e) => {
                            let t = trf(
                                lang,
                                K_EXPORT_RECIPE_SAVED_BUT_THE_LIGHTROOM_XMP_FAILED,
                                &[("err", &e.to_string())],
                            );
                            self.workspace.toast(ToastKind::Error, t.clone());
                            s = t;
                        }
                    }
                }
                s
            }
            Err(e) => {
                // The old save was already COPIED
                // to v<N>, so nothing is lost —
                // but this photo's working save
                // failed, and a status line alone
                // scrolls away.
                let t = self.workspace.show_words(
                    lang,
                    logic::analysis_landing::words(
                        &logic::analysis_landing::SaveReport::SidecarFailed {
                            err: e.to_string(),
                        },
                    ),
                );
                self.refresh_versions();
                t
            }
        }
    }
        })
    },
) {
    Ok(status) => status,
    Err(e) => {
        self.workspace.persist_postponed(self.lang, 
            &e,
            "AI develop applied — but NOT saved: this photo is being changed by another AutoShade process ({err}); Ctrl+S retries",
            &[],
        );
        self.workspace.status().clone()
    }
}
    }
}
