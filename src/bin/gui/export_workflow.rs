//! The export landing workflow: classify the worker result and compose its
//! line. The host applies status/toast/done, repairs the canvas and remembers
//! the destination.

use crate::*;

/// What the export landing asks the host to do.
pub(crate) enum ExportLanding {
    Failed(anyhow::Error),
    Single {
        relooked: bool,
        line: String,
        /// The line carries a mask warning: it must use the error channel.
        error: bool,
    },
    Batch {
        ok: usize,
        dest: PathBuf,
        line: String,
        /// At least one target failed: it must not read as a clean success.
        error: bool,
    },
}

pub(crate) fn land(lang: Lang, res: anyhow::Result<ExportOutcome>) -> ExportLanding {
    match res {
        Err(e) => ExportLanding::Failed(e),
        Ok(ExportOutcome::Single { out, relooked, warn }) => {
            let words = logic::export_landing::single_words(&abs_display(&out), relooked, warn.as_deref());
            let mut line = crate::i18n::text(lang, &words.line);
            if let Some(relook) = words.relook {
                line = format!("{line} — {}", crate::i18n::text(lang, &relook));
            }
            let error = words.warn.is_some();
            if let Some(warn) = words.warn {
                line = format!("{line} · {}", crate::i18n::text(lang, &warn));
            }
            ExportLanding::Single { relooked, line, error }
        }
        Ok(ExportOutcome::Batch { ok, errs, renamed, relooked, warns, dest }) => {
            let words = logic::export_landing::batch_words(
                ok,
                &errs,
                &renamed,
                relooked,
                &warns,
                &abs_display(&dest),
            );
            let mut line = crate::i18n::text(lang, &words.base);
            for part in [words.renamed, words.relooked, words.develops].into_iter().flatten() {
                line.push_str(&crate::i18n::text(lang, &part));
            }
            ExportLanding::Batch { ok, dest, line, error: !errs.is_empty() }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single(warn: Option<&str>) -> ExportOutcome {
        ExportOutcome::Single { out: PathBuf::from("out.jpg"), relooked: true, warn: warn.map(str::to_owned) }
    }

    fn batch(errs: Vec<String>) -> ExportOutcome {
        ExportOutcome::Batch {
            ok: 2,
            errs,
            renamed: Vec::new(),
            relooked: 0,
            warns: Vec::new(),
            dest: PathBuf::from("out"),
        }
    }

    #[test]
    fn a_clean_single_uses_the_success_channel() {
        match land(Lang::En, Ok(single(None))) {
            ExportLanding::Single { line, error, relooked } => {
                assert!(!error);
                assert!(relooked);
                assert!(line.contains("exported"));
            }
            _ => panic!("expected a single landing"),
        }
    }

    #[test]
    fn a_mask_warning_uses_the_error_channel() {
        match land(Lang::En, Ok(single(Some("bitmap mask missing")))) {
            ExportLanding::Single { line, error, .. } => {
                assert!(error);
                assert!(line.contains("bitmap mask missing"));
            }
            _ => panic!("expected a single landing"),
        }
    }

    #[test]
    fn a_clean_batch_carries_its_destination() {
        match land(Lang::En, Ok(batch(Vec::new()))) {
            ExportLanding::Batch { ok, dest, error, .. } => {
                assert_eq!(ok, 2);
                assert_eq!(dest, PathBuf::from("out"));
                assert!(!error);
            }
            _ => panic!("expected a batch landing"),
        }
    }

    #[test]
    fn a_partial_batch_uses_the_error_channel() {
        match land(Lang::En, Ok(batch(vec!["b.jpg: locked".into()]))) {
            ExportLanding::Batch { error, .. } => assert!(error),
            _ => panic!("expected a batch landing"),
        }
    }

    #[test]
    fn a_worker_error_is_forwarded() {
        assert!(matches!(land(Lang::En, Err(anyhow::anyhow!("disk full"))), ExportLanding::Failed(_)));
    }
}
