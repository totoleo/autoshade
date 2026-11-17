//! The develop/redevelop workflow: when to dispatch a preview frame, and
//! whether a landed frame still describes the live base + recipe.
//!
//! The worker and the texture upload stay host responsibilities; this module
//! owns the two decisions that used to sit inside `start_redevelop` and
//! `finish_redevelop`.

use std::sync::Arc;

use crate::*;

/// What the frame gate asks the host to do.
pub(crate) enum DevelopDispatch {
    /// Nothing to do: a frame is already in flight, the base part is not
    /// ready, or there are no pixels to develop.
    Skip,
    /// The source frame cannot develop this recipe; the host words it.
    Fail(String),
    /// Start one worker with this snapshot.
    Start {
        base: Arc<image::DynamicImage>,
        recipe: Box<EditRecipe>,
        show_clipping: bool,
        film: Option<u32>,
        source: autoshade::render::SourceFrame,
    },
}

/// Decide whether to start a preview worker. `source_part_ready` is the
/// host's `ensure_source_part()` answer; `source` is the host's checked
/// source frame, already formatted for the status line.
pub(crate) fn plan_dispatch(
    inflight: bool,
    source_part_ready: bool,
    base: Option<Arc<image::DynamicImage>>,
    recipe: &EditRecipe,
    show_clipping: bool,
    film: Option<u32>,
    source: Result<autoshade::render::SourceFrame, String>,
) -> DevelopDispatch {
    if inflight || !source_part_ready {
        return DevelopDispatch::Skip;
    }
    let Some(base) = base else {
        return DevelopDispatch::Skip;
    };
    let source = match source {
        Ok(source) => source,
        Err(e) => return DevelopDispatch::Fail(e),
    };
    DevelopDispatch::Start { base, recipe: Box::new(recipe.clone()), show_clipping, film, source }
}

/// Is a landed frame stale? It is current only when it was solved for the
/// live base allocation AND the live recipe.
pub(crate) fn is_stale(
    frame: &PreviewDone,
    current_base: Option<&Arc<image::DynamicImage>>,
    current_recipe: &EditRecipe,
) -> bool {
    !(current_base.is_some_and(|base| Arc::ptr_eq(base, &frame.base)) && current_recipe == &frame.recipe)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Arc<image::DynamicImage> {
        Arc::new(image::DynamicImage::new_rgb8(1, 1))
    }

    fn source() -> autoshade::render::SourceFrame {
        autoshade::render::SourceFrame::Baked(None)
    }

    fn frame(base: Arc<image::DynamicImage>, recipe: EditRecipe) -> PreviewDone {
        PreviewDone {
            base,
            recipe,
            rgb: image::RgbImage::new(1, 1),
            after: egui::ColorImage::new([1, 1], vec![egui::Color32::BLACK]),
            histogram: Vec::new(),
            clipping: None,
            thumb: egui::ColorImage::new([1, 1], vec![egui::Color32::BLACK]),
        }
    }

    #[test]
    fn an_inflight_frame_or_unready_part_is_skipped() {
        assert!(matches!(plan_dispatch(true, true, Some(base()), &EditRecipe::default(), false, None, Ok(source())), DevelopDispatch::Skip));
        assert!(matches!(plan_dispatch(false, false, Some(base()), &EditRecipe::default(), false, None, Ok(source())), DevelopDispatch::Skip));
        assert!(matches!(plan_dispatch(false, true, None, &EditRecipe::default(), false, None, Ok(source())), DevelopDispatch::Skip));
    }

    #[test]
    fn a_source_error_is_reported() {
        let cmd = plan_dispatch(false, true, Some(base()), &EditRecipe::default(), false, None, Err("wrong lens".into()));
        match cmd {
            DevelopDispatch::Fail(e) => assert_eq!(e, "wrong lens"),
            _ => panic!("expected a failure"),
        }
    }

    #[test]
    fn a_ready_dispatch_carries_the_snapshot() {
        let recipe = EditRecipe { exposure_ev: 0.4, ..Default::default() };
        let cmd = plan_dispatch(false, true, Some(base()), &recipe, true, Some(1024), Ok(source()));
        match cmd {
            DevelopDispatch::Start { recipe: r, show_clipping, film, .. } => {
                assert_eq!(*r, recipe);
                assert!(show_clipping);
                assert_eq!(film, Some(1024));
            }
            _ => panic!("expected a dispatch"),
        }
    }

    #[test]
    fn a_frame_is_stale_when_its_base_or_recipe_moved() {
        let b = base();
        let recipe = EditRecipe::default();
        let done = frame(b.clone(), recipe.clone());
        assert!(!is_stale(&done, Some(&b), &recipe), "same base and recipe is current");
        assert!(is_stale(&done, None, &recipe), "no base is stale");
        assert!(is_stale(&done, Some(&base()), &recipe), "a different allocation is stale");
        let changed = EditRecipe { exposure_ev: 1.0, ..Default::default() };
        assert!(is_stale(&done, Some(&b), &changed), "a changed recipe is stale");
    }
}
