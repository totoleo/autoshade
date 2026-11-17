//! The analysis landing workflow: the epoch guard and the landing decision,
//! as data the host executes.
//!
//! The worker's result is either stale (discard), failed (word it), or a
//! landing to install. The host still owns the commit/fork/persist sequence;
//! this module owns WHICH sequence is due and what the landing contains.

use std::path::PathBuf;

use autoshade::advisor::Verdict;
use autoshade::rationale::Note;
use autoshade::recipe::EditRecipe;

/// What the analysis landing asks the host to do.
pub(crate) enum AnalysisCommand {
    /// A cancelled or superseded result: never install it.
    Ignore,
    /// Install this landing, then persist `stamped` when `accepted`.
    Install(Box<AnalysisInstall>),
    /// The worker failed; the host words the error.
    Failed(anyhow::Error),
}

/// One analysis landing's payload: the canvas recipe, the store's stamped
/// recipe, the verdict and its notes.
pub(crate) struct AnalysisInstall {
    pub(crate) canvas: EditRecipe,
    pub(crate) stamped: EditRecipe,
    pub(crate) verdict: Verdict,
    pub(crate) notes: Vec<Note>,
    pub(crate) accepted: bool,
    pub(crate) photo: Option<PathBuf>,
}

/// Decide one worker result. `epoch` is the epoch the worker ran under;
/// `current_epoch` is the app's live one.
pub(crate) fn land(
    epoch: u64,
    current_epoch: u64,
    result: anyhow::Result<(EditRecipe, Verdict, Vec<Note>)>,
    skin_held: bool,
    ai_pixels: bool,
    photo: Option<PathBuf>,
) -> AnalysisCommand {
    if epoch != current_epoch {
        return AnalysisCommand::Ignore;
    }
    match result {
        Ok((recipe, verdict, notes)) => {
            let shaped = super::analysis_landing::shape(recipe, skin_held, ai_pixels);
            AnalysisCommand::Install(Box::new(AnalysisInstall {
                canvas: shaped.canvas,
                stamped: shaped.stamped,
                accepted: super::analysis_landing::accepted(&verdict),
                verdict,
                notes,
                photo,
            }))
        }
        Err(e) => AnalysisCommand::Failed(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autoshade::advisor::Decision;

    fn verdict(decision: Decision) -> Verdict {
        Verdict { decision, reasons: Vec::new(), revised_hint: None }
    }

    /// A late result from a cancelled request never reaches the canvas.
    #[test]
    fn a_stale_epoch_is_ignored() {
        let cmd = land(1, 2, Ok((EditRecipe::default(), verdict(Decision::Accept), Vec::new())), false, false, None);
        assert!(matches!(cmd, AnalysisCommand::Ignore));
    }

    /// An accepted landing carries both recipes and is allowed to persist.
    #[test]
    fn an_accepted_landing_is_installed() {
        let recipe = EditRecipe { exposure_ev: 0.2, ..Default::default() };
        let cmd = land(3, 3, Ok((recipe.clone(), verdict(Decision::Accept), Vec::new())), false, false, None);
        match cmd {
            AnalysisCommand::Install(install) => {
                let AnalysisInstall { canvas, stamped, accepted, .. } = *install;
                assert!(accepted, "Accept may auto-save");
                assert_eq!(canvas, recipe);
                assert_eq!(stamped, recipe);
            }
            _ => panic!("expected an install"),
        }
    }

    /// A non-Accept verdict lands on the canvas but is not persisted.
    #[test]
    fn a_revise_verdict_stays_unsaved() {
        let cmd = land(3, 3, Ok((EditRecipe::default(), verdict(Decision::Revise), Vec::new())), false, false, None);
        match cmd {
            AnalysisCommand::Install(install) => assert!(!install.accepted, "only Accept may auto-save"),
            _ => panic!("expected an install"),
        }
    }

    /// A worker error is forwarded, not turned into a silent landing.
    #[test]
    fn an_error_is_forwarded() {
        let cmd = land(3, 3, Err(anyhow::anyhow!("no key")), false, false, None);
        assert!(matches!(cmd, AnalysisCommand::Failed(_)));
    }
}
