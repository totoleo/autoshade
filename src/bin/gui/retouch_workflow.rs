//! The retouch landing workflow: classify a worker result into the landing
//! shape the host installs.
//!
//! The worker returns pixels, a typed note, the saved master and the kind of
//! card the result belongs on. This module owns that mapping and the HDR
//! headroom rule; the host owns the commit/fork/switch/persist sequence.

use std::path::PathBuf;
use std::sync::Arc;

use crate::*;
use crate::logic::retouch::RetouchNote;

/// What a retouch result asks the host to install.
pub(crate) enum RetouchLanding {
    /// A cancelled or superseded result. `discard` is the pixel master the
    /// worker already saved; the host removes it so a cancel leaves no orphan.
    Ignore { discard: Option<PathBuf> },
    /// The worker failed; the host words the error.
    Failed(anyhow::Error),
    /// A new card whose base is the retouched raster.
    NewVariant {
        kind: VariantKind,
        recipe: Box<EditRecipe>,
        image: Arc<image::DynamicImage>,
        origin: PathBuf,
        note: RetouchNote,
    },
    /// A layer over the card on screen.
    Layer { path: PathBuf, note: RetouchNote },
    /// A pixel touch-up of the active card's base.
    InPlace { image: Arc<image::DynamicImage>, origin: PathBuf, note: RetouchNote },
}

/// Decide one retouch landing. `epoch` is the epoch the worker ran under;
/// `current_epoch` is the app's live one.
pub(crate) fn land(
    epoch: u64,
    current_epoch: u64,
    done: RetouchDone,
    live_recipe: &EditRecipe,
) -> RetouchLanding {
    if epoch != current_epoch {
        let discard = done.ok().map(|(_, _, saved, _)| saved);
        return RetouchLanding::Ignore { discard };
    }
    match done {
        Ok((img, note, saved, kind)) => match kind {
            RetouchKind::NewGenerated => RetouchLanding::NewVariant {
                kind: VariantKind::Generated,
                recipe: Box::new(EditRecipe::default()),
                image: Arc::new(img),
                origin: saved,
                note,
            },
            RetouchKind::Layer => RetouchLanding::Layer {
                // Absolute, so the saved recipe finds it from any working
                // directory.
                path: std::path::absolute(&saved).unwrap_or(saved),
                note,
            },
            RetouchKind::NewDenoised | RetouchKind::NewStacked => {
                let mut recipe = live_recipe.clone();
                // An HDR merge hands over stops of highlight above the
                // reference frame's white. They are only reachable through
                // the SDR rendition stage, so the card arrives with that
                // stage ON and told how much room it has.
                if let RetouchNote::Stacked { headroom_ev, .. } = &note
                    && *headroom_ev > 0.0
                {
                    recipe.hdr_edit = true;
                    recipe.hdr_max_ev = *headroom_ev;
                }
                RetouchLanding::NewVariant {
                    kind: if kind == RetouchKind::NewStacked {
                        VariantKind::Stacked
                    } else {
                        VariantKind::Denoised
                    },
                    recipe: Box::new(recipe),
                    image: Arc::new(img),
                    origin: saved,
                    note,
                }
            }
            RetouchKind::InPlace => RetouchLanding::InPlace {
                image: Arc::new(img),
                origin: saved,
                note,
            },
        },
        Err(e) => RetouchLanding::Failed(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> image::DynamicImage {
        image::DynamicImage::new_rgb8(1, 1)
    }

    fn note() -> RetouchNote {
        RetouchNote::Denoised { out: PathBuf::from("out.png"), on_mosaic: false }
    }

    #[test]
    fn a_stale_success_is_ignored_and_its_master_is_discarded() {
        let saved = PathBuf::from("master.png");
        let landing = land(1, 2, Ok((image(), note(), saved.clone(), RetouchKind::InPlace)), &EditRecipe::default());
        match landing {
            RetouchLanding::Ignore { discard } => assert_eq!(discard, Some(saved)),
            _ => panic!("expected an ignore"),
        }
    }

    #[test]
    fn a_stale_error_is_ignored_without_a_file() {
        let landing = land(1, 2, Err(anyhow::anyhow!("cancel")), &EditRecipe::default());
        match landing {
            RetouchLanding::Ignore { discard } => assert_eq!(discard, None),
            _ => panic!("expected an ignore"),
        }
    }

    #[test]
    fn a_reimagine_lands_as_a_generated_card() {
        let landing = land(3, 3, Ok((image(), note(), PathBuf::from("out.png"), RetouchKind::NewGenerated)), &EditRecipe::default());
        match landing {
            RetouchLanding::NewVariant { kind, recipe, .. } => {
                assert_eq!(kind, VariantKind::Generated);
                assert_eq!(*recipe, EditRecipe::default());
            }
            _ => panic!("expected a new variant"),
        }
    }

    #[test]
    fn an_hdr_stack_carries_its_headroom() {
        let note = RetouchNote::Stacked {
            out: PathBuf::from("stack.png"),
            kind: autoshade::stack::merge::StackKind::Hdr,
            frames: 3,
            travel: 0.0,
            uncovered: 0.0,
            headroom_ev: 1.5,
        };
        let landing = land(3, 3, Ok((image(), note, PathBuf::from("stack.png"), RetouchKind::NewStacked)), &EditRecipe::default());
        match landing {
            RetouchLanding::NewVariant { kind, recipe, .. } => {
                assert_eq!(kind, VariantKind::Stacked);
                assert!(recipe.hdr_edit);
                assert_eq!(recipe.hdr_max_ev, 1.5);
            }
            _ => panic!("expected a new variant"),
        }
    }

    #[test]
    fn a_fill_lands_as_a_layer() {
        let landing = land(3, 3, Ok((image(), note(), PathBuf::from("patch.png"), RetouchKind::Layer)), &EditRecipe::default());
        match landing {
            RetouchLanding::Layer { path, .. } => assert!(path.is_absolute()),
            _ => panic!("expected a layer"),
        }
    }

    #[test]
    fn a_heal_lands_in_place() {
        let landing = land(3, 3, Ok((image(), note(), PathBuf::from("heal.png"), RetouchKind::InPlace)), &EditRecipe::default());
        assert!(matches!(landing, RetouchLanding::InPlace { .. }));
    }
}
