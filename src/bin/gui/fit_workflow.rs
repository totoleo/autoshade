//! The reverse-fit landing workflow: turn the worker's outcome into the
//! variant the host installs, including the "same negative" base rule.

use std::path::PathBuf;
use std::sync::Arc;

use crate::*;

/// What the fit landing asks the host to install.
pub(crate) enum FitLanding {
    Failed(anyhow::Error),
    Install {
        persisted: bool,
        recipe: Box<EditRecipe>,
        err_before: f32,
        err_after: f32,
        status: Vec<FitNote>,
        rationale_notes: Vec<autoshade::rationale::Note>,
        negative: Option<PathBuf>,
        /// The negative's decoded base when the original card still carries
        /// the same master; otherwise the host re-decodes from `origin`.
        base: Option<Arc<image::DynamicImage>>,
    },
}

/// Classify one fit result. `original` is the photo's Original card, when
/// there is one; the ◭ card shares its decoded pixels only when that card
/// still hangs off the same negative master.
pub(crate) fn land(out: anyhow::Result<FitOutcome>, original: Option<&Variant>) -> FitLanding {
    match out {
        Err(e) => FitLanding::Failed(e),
        Ok(out) => {
            let base = out.negative.as_deref().and_then(|m| {
                let v = original?;
                (v.origin.as_deref() == Some(m)).then(|| v.base.clone()).flatten()
            });
            FitLanding::Install {
                persisted: out.persisted,
                recipe: Box::new(out.recipe),
                err_before: out.err_before,
                err_after: out.err_after,
                status: out.status,
                rationale_notes: out.rationale_notes,
                negative: out.negative,
                base,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(negative: Option<PathBuf>) -> FitOutcome {
        FitOutcome {
            recipe: EditRecipe::default(),
            err_before: 1.0,
            err_after: 0.5,
            rationale_notes: Vec::new(),
            status: Vec::new(),
            persisted: true,
            negative,
        }
    }

    fn original(origin: Option<PathBuf>, base: Option<Arc<image::DynamicImage>>) -> Variant {
        Variant {
            kind: VariantKind::Original,
            id: "original".into(),
            name: None,
            recipe: EditRecipe::default(),
            base,
            origin,
            thumb: None,
        }
    }

    #[test]
    fn an_error_is_forwarded() {
        assert!(matches!(land(Err(anyhow::anyhow!("no key")), None), FitLanding::Failed(_)));
    }

    #[test]
    fn a_same_negative_shares_the_originals_base() {
        let master = PathBuf::from("master.png");
        let base = Arc::new(image::DynamicImage::new_rgb8(1, 1));
        let orig = original(Some(master.clone()), Some(base.clone()));
        let landing = land(Ok(outcome(Some(master))), Some(&orig));
        match landing {
            FitLanding::Install { base: b, persisted, .. } => {
                assert!(persisted);
                assert!(b.is_some_and(|b| Arc::ptr_eq(&b, &base)));
            }
            _ => panic!("expected an install"),
        }
    }

    #[test]
    fn a_different_negative_leaves_the_base_for_the_host_to_decode() {
        let landing = land(
            Ok(outcome(Some(PathBuf::from("other.png")))),
            Some(&original(Some(PathBuf::from("master.png")), None)),
        );
        match landing {
            FitLanding::Install { base, .. } => assert!(base.is_none()),
            _ => panic!("expected an install"),
        }
    }

    #[test]
    fn no_negative_has_no_base() {
        let landing = land(Ok(outcome(None)), Some(&original(None, None)));
        match landing {
            FitLanding::Install { base, .. } => assert!(base.is_none()),
            _ => panic!("expected an install"),
        }
    }
}
