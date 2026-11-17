//! The analysis landing: the epoch guard, the landing shape, the state install
//! and the call into `landing_persist` for the write half. `workers.rs` keeps
//! the message pump.

use crate::*;

impl Commands<'_> {
/// `Msg::Opened` landing — body extracted verbatim from the
/// poll_workers pump (round-12 decomposition; indentation kept).
    /// `Msg::Analyzed` landing — body extracted verbatim from the
/// poll_workers pump (round-12 decomposition; indentation kept).
pub(crate) fn on_analyzed(
    &mut self,
    lang: Lang,
    epoch: u64,
    boxed: Box<
        anyhow::Result<(
            EditRecipe,
            autoshade::advisor::Verdict,
            Vec<autoshade::rationale::Note>,
        )>,
    >,
) {
                // Cleared whatever the epoch: the wire is free either way,
                // and this is what re-arms the Analyze button.
                self.workspace.tools_mut().analyze_inflight = false;
                if epoch != self.workspace.tools().gen_epoch {
                    // A cancelled Analyze's late result: the user already
                    // moved on. The Ok arm below also PERSISTS (backup +
                    // recipe.json + XMP) — a stale install would not just
                    // repaint the canvas, it would save over the photo's
                    // develop. Discard entirely (Err is just as silent).
                    return; // was `continue` — the match was the loop's last statement
                }
                self.workspace.tools_mut().gen_cancel = None;
                match *boxed {
                Ok((recipe, verdict, rationale_notes)) => {
                    // Sliders stay live while Analyze runs (10-30 s):
                    // flush any name typed during the wait (the resync
                    // below clears the mask buffer — unflushed, the
                    // rename died with it, M16), then commit any
                    // mid-flight edit as its own undo step NOW, or the
                    // wholesale install below folds it into the
                    // pre-analyze step and Ctrl+Z skips it.
                    self.commit_pending_names();
                    self.editor.commit_now(&self.workspace);
                    let accepted = logic::analysis_landing::accepted(&verdict);
                    // The recipe arrives with its base_curve already
                    // stamped by produce_recipe (saved-curve-first, else a
                    // fresh estimate) — the single authority all three
                    // surfaces share; the canvas must not override it.
                    // One exception, canvas-side only: a GENERATED variant
                    // already carries the camera look AND the lens
                    // corrections in its pixels, so developing it through
                    // the RAW's curve would cook both twice — the canvas
                    // copy drops them; the persist below still writes the
                    // full stamped recipe (it is the RAW's saved develop,
                    // and reopening lands on the source). An InPlace
                    // retouch master is a NEUTRAL develop (see the
                    // RetouchKind::InPlace arm): its recipe legitimately
                    // keeps the calibration — stripping there rendered the
                    // canvas neutral-dark while the disk recipe kept the
                    // look.
                    // The landing's two recipes: the canvas copy a
                    // GENERATED card needs (baked look stripped), and the
                    // store's own (the RAW's develop). See
                    // `logic::analysis_landing::shape`.
                    let shaped = logic::analysis_landing::shape(
                        recipe,
                        self.editor.skin_held(&self.workspace),
                        self.editor.active_on_ai_pixels(),
                    );
                    let stamped = shaped.stamped;
                    self.editor.doc_mut().recipe = shaped.canvas;
                    // The immutability rule, BEFORE the persist below
                    // reads the strip: an analysis landing on the
                    // pristine ✨ card is an edit, and lands on a new ✎
                    // card — the record this landing writes names it.
                    self.fork_edited_card();
                    // Wholesale replacement: disarm index-carrying tools +
                    // refresh rationale, THEN install the fresh verdict
                    // and the typed notes (resync cleared both).
                    self.editor.resync_recipe_display(&mut self.workspace);
                    self.editor.analysis_mut().rationale_notes = rationale_notes;
                    self.editor.analysis_mut().verdict =
                        Some((verdict.decision.clone(), verdict.reasons.clone()));
                    self.editor.session_mut().dirty = true;
                    self.workspace.set_busy(false);
                    // Persist like the CLI and the web do — the badge,
                    // batch render and reopening all read recipe.json, so
                    // an unpersisted analyze silently diverges from every
                    // one of them. An existing explicit save is snapshotted
                    // to v<N> first, never destroyed.
                    *self.workspace.status_mut() = if !accepted {
                        // The verifier itself judged this result not
                        // ready, and a non-Accept verdict may not
                        // auto-save (user decision). The result stays on
                        // the canvas as an UNSAVED edit — ● lit, the nav
                        // stash and the close guard protect it — and the
                        // user decides: Ctrl+S keeps it, Ctrl+Z steps
                        // back to the pre-analyze edit.
                        self.workspace.show_words(
                            lang,
                            logic::analysis_landing::words(
                                &logic::analysis_landing::SaveReport::NotAccepted {
                                    decision: autoshade::advisor::decision_key(&verdict.decision),
                                },
                            ),
                        )
                    } else { match self.editor.photo().src_path.clone() {
                        // The backup gate comes FIRST: if the existing save
                        // cannot be snapshotted, it is not overwritten —
                        // the analyze result stays on the canvas only.
                        // Persist `stamped`, not the canvas copy: the
                        // sidecar is the RAW's develop and keeps its curve.
                        // ONE NoWait lock across gate → recipe → pixel
                        // link → XMP (the Ctrl+S pairing): busy must
                        // degrade to "applied but NOT saved", never hang
                        // the UI thread behind another process.
                        Some(p) => self.persist_analysis_landing(lang, p, stamped),
                        None => self.workspace.show_words(
                            lang,
                            logic::analysis_landing::words(&logic::analysis_landing::SaveReport::NoPhoto),
                        ),
                    } };
                }
                Err(e) => {
                    self.workspace.fail(tr(lang, K_LANDING_ANALYZE_FAILED), e);
                }
            }
}
}
