//! The paste landing workflow: a partial failure must never read as a clean
//! success, and only a clean landing may advance the ● baseline.

use crate::*;

/// What the paste worker's result asks the host to install.
pub(crate) enum PasteLanding {
    /// Every target was written and every XMP half succeeded.
    Clean(PasteOutcome),
    /// At least one target failed; the others may still have been written.
    Partial(PasteOutcome),
    /// The worker itself failed.
    Failed(anyhow::Error),
}

pub(crate) fn land(res: anyhow::Result<PasteOutcome>) -> PasteLanding {
    match res {
        Ok(out) if out.errs.is_empty() => PasteLanding::Clean(out),
        Ok(out) => PasteLanding::Partial(out),
        Err(e) => PasteLanding::Failed(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(errs: Vec<String>) -> PasteOutcome {
        PasteOutcome {
            ok: 2,
            xmp: 1,
            errs,
            xmp_fails: Vec::new(),
            xmp_notes: Vec::new(),
            written: vec![PathBuf::from("a.jpg")],
        }
    }

    #[test]
    fn a_clean_outcome_is_clean() {
        assert!(matches!(land(Ok(outcome(Vec::new()))), PasteLanding::Clean(_)));
    }

    #[test]
    fn a_partial_outcome_is_not_a_clean_success() {
        assert!(matches!(land(Ok(outcome(vec!["b.jpg: locked".into()]))), PasteLanding::Partial(_)));
    }

    #[test]
    fn a_worker_error_is_forwarded() {
        assert!(matches!(land(Err(anyhow::anyhow!("no selection"))), PasteLanding::Failed(_)));
    }

    #[test]
    fn the_written_list_survives_either_outcome() {
        for landing in [land(Ok(outcome(Vec::new()))), land(Ok(outcome(vec!["x".into()])))] {
            let out = match landing {
                PasteLanding::Clean(out) | PasteLanding::Partial(out) => out,
                PasteLanding::Failed(_) => panic!("not a landing"),
            };
            assert_eq!(out.written, vec![PathBuf::from("a.jpg")]);
        }
    }
}
