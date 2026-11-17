//! The analysis landing's host half: install the workflow's decision, then
//! persist the stamped recipe. The epoch guard and the landing shape live in
//! `logic::analysis_workflow`; the write half lives in `landing_persist`.

use crate::*;

impl Commands<'_> {
    /// `Msg::Analyzed` landing.
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
        // Cleared whatever the epoch: the wire is free either way, and this is
        // what re-arms the Analyze button.
        self.workspace.tools_mut().analyze_inflight = false;
        let command = logic::analysis_workflow::land(
            epoch,
            self.workspace.tools().gen_epoch,
            *boxed,
            self.editor.skin_held(self.workspace),
            self.editor.active_on_ai_pixels(),
            self.editor.photo().src_path.clone(),
        );
        if !matches!(&command, logic::analysis_workflow::AnalysisCommand::Ignore) {
            self.workspace.tools_mut().gen_cancel = None;
        }
        match command {
            logic::analysis_workflow::AnalysisCommand::Ignore => {
                // A cancelled or superseded Analyze: never install or save it.
            }
            logic::analysis_workflow::AnalysisCommand::Failed(e) => {
                self.workspace.fail(tr(lang, "analyze failed"), e);
            }
            logic::analysis_workflow::AnalysisCommand::Install(install) => {
                let logic::analysis_workflow::AnalysisInstall {
                canvas,
                stamped,
                verdict,
                notes,
                accepted,
                photo,
                } = *install;
                // Sliders stay live while Analyze runs (10-30 s): flush any
                // name typed during the wait, then commit a mid-flight edit as
                // its own undo step before the wholesale install below.
                self.commit_pending_names();
                self.editor.commit_now(self.workspace);
                self.editor.doc_mut().recipe = canvas;
                // The immutability rule, BEFORE the persist below reads the
                // strip: an analysis landing on the pristine ✨ card is an
                // edit, and lands on a new ✎ card.
                self.fork_edited_card();
                // Wholesale replacement: disarm index-carrying tools and
                // refresh the rationale, then install the fresh verdict.
                self.editor.resync_recipe_display(self.workspace);
                self.editor.analysis_mut().rationale_notes = notes;
                self.editor.analysis_mut().verdict =
                    Some((verdict.decision.clone(), verdict.reasons.clone()));
                self.editor.session_mut().dirty = true;
                self.workspace.set_busy(false);
                *self.workspace.status_mut() = if !accepted {
                    // The verifier judged this result not ready: it stays on
                    // the canvas as an unsaved edit and the user decides.
                    self.workspace.show_words(
                        lang,
                        logic::analysis_landing::words(
                            &logic::analysis_landing::SaveReport::NotAccepted {
                                decision: autoshade::advisor::decision_key(&verdict.decision),
                            },
                        ),
                    )
                } else {
                    match photo {
                        // Persist `stamped`, not the canvas copy: the sidecar
                        // is the RAW's develop and keeps its curve.
                        Some(p) => self.persist_analysis_landing(lang, p, stamped),
                        None => self.workspace.show_words(
                            lang,
                            logic::analysis_landing::words(&logic::analysis_landing::SaveReport::NoPhoto),
                        ),
                    }
                };
            }
        }
    }
}
