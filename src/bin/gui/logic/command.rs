//! The DOMAIN command layer: the mask list's verbs as data intents, applied in
//! one place. A scalar control's write is not here — the control registry
//! (`controls.rs`) owns it, because a control's behaviour is polymorphic and a
//! `Command` should stay a plain, serialisable intent, not a function pointer.

use autoshade::recipe::EditRecipe;

/// One edit the GUI asks the document to make.
#[derive(Clone, Debug)]
pub(crate) enum Command {
    /// A local-adjustment list operation (the mask panel's verbs).
    Mask(MaskCommand),
    /// Insert `mask` after slot `after` (the duplicate verb: the caller has
    /// already detached the rasters and named the copy).
    InsertMask { after: usize, mask: Box<autoshade::recipe::LocalAdjustment> },
}

/// The mask-list verbs a client can ask for. Selection remapping and the
/// overlay invalidation stay with the caller: this layer owns the recipe list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MaskCommand {
    /// Drop the mask at `index` (out of range: nothing).
    Delete { index: usize },
    /// Flip the mask's eye (out of range: nothing).
    ToggleEnabled { index: usize },
    /// Move `from` to the final slot `to` (both pre-checked by the caller).
    Reorder { from: usize, to: usize },
    /// Swap the mask with the one above it (the first row: nothing).
    MoveUp { index: usize },
    /// Swap the mask with the one below it (the last row: nothing).
    MoveDown { index: usize },
    /// Replace the mask's display name (out of range: nothing).
    Rename { index: usize, name: String },
    /// Replace the mask's range mode (the panel builds the neutral start).
    SetRange { index: usize, range: Option<autoshade::recipe::RangeMask> },
    /// Replace one component's combine mode.
    SetComponentMode { index: usize, component: usize, mode: autoshade::recipe::MaskCombine },
    /// Replace one component's inverted flag.
    SetComponentInverted { index: usize, component: usize, inverted: bool },
    /// Drop one component.
    RemoveComponent { index: usize, component: usize },
}

/// Apply one command to the recipe. The GUI calls this now; the CLI and the
/// web form are meant to call the same function.
pub(crate) fn apply(recipe: &mut EditRecipe, command: Command) {
    match command {
        Command::Mask(MaskCommand::Delete { index }) => {
            if index < recipe.masks.len() {
                recipe.masks.remove(index);
            }
        }
        Command::Mask(MaskCommand::ToggleEnabled { index }) => {
            if let Some(m) = recipe.masks.get_mut(index) {
                m.enabled = !m.enabled;
            }
        }
        Command::Mask(MaskCommand::Reorder { from, to }) => {
            if from < recipe.masks.len() && to < recipe.masks.len() {
                let m = recipe.masks.remove(from);
                recipe.masks.insert(to, m);
            }
        }
        Command::Mask(MaskCommand::MoveUp { index }) => {
            if index > 0 && index < recipe.masks.len() {
                recipe.masks.swap(index, index - 1);
            }
        }
        Command::Mask(MaskCommand::MoveDown { index }) => {
            if index + 1 < recipe.masks.len() {
                recipe.masks.swap(index, index + 1);
            }
        }
        Command::Mask(MaskCommand::Rename { index, name }) => {
            if let Some(m) = recipe.masks.get_mut(index) {
                m.name = name;
            }
        }
        Command::Mask(MaskCommand::SetRange { index, range }) => {
            if let Some(m) = recipe.masks.get_mut(index) {
                m.range = range;
            }
        }
        Command::Mask(MaskCommand::SetComponentMode { index, component, mode }) => {
            if let Some(c) = recipe.masks.get_mut(index).and_then(|m| m.components.get_mut(component)) {
                c.mode = mode;
            }
        }
        Command::Mask(MaskCommand::SetComponentInverted { index, component, inverted }) => {
            if let Some(c) = recipe.masks.get_mut(index).and_then(|m| m.components.get_mut(component)) {
                c.inverted = inverted;
            }
        }
        Command::Mask(MaskCommand::RemoveComponent { index, component }) => {
            if let Some(m) = recipe.masks.get_mut(index)
                && component < m.components.len() {
                    m.components.remove(component);
                }
        }
        Command::InsertMask { after, mask } => {
            // The recipe's own cap (64) is a domain invariant, so it is
            // enforced HERE as well as by the panel's toast.
            if recipe.masks.len() < 64 {
                recipe.masks.insert((after + 1).min(recipe.masks.len()), *mask);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn the_mask_list_verbs_edit_the_recipe_and_ignore_stale_indices() {
        use autoshade::recipe::LocalAdjustment;
        let mut recipe = EditRecipe::default();
        for name in ["a", "b", "c"] {
            recipe.masks.push(LocalAdjustment { name: name.into(), ..Default::default() });
        }
        apply(&mut recipe, Command::Mask(MaskCommand::ToggleEnabled { index: 1 }));
        assert!(!recipe.masks[1].enabled);
        apply(&mut recipe, Command::Mask(MaskCommand::Reorder { from: 0, to: 2 }));
        assert_eq!(recipe.masks.iter().map(|m| m.name.trim()).collect::<Vec<_>>(), ["b", "c", "a"]);
        apply(&mut recipe, Command::Mask(MaskCommand::Delete { index: 1 }));
        assert_eq!(recipe.masks.iter().map(|m| m.name.trim()).collect::<Vec<_>>(), ["b", "a"]);
        // A stale index from a drag started before the list shrank is a no-op,
        // not a panic.
        apply(&mut recipe, Command::Mask(MaskCommand::Delete { index: 9 }));
        apply(&mut recipe, Command::Mask(MaskCommand::ToggleEnabled { index: 9 }));
        apply(&mut recipe, Command::Mask(MaskCommand::Reorder { from: 9, to: 0 }));
        assert_eq!(recipe.masks.len(), 2);
    }

    #[test]
    fn the_component_and_rename_verbs_edit_only_the_named_slot() {
        use autoshade::recipe::{LocalAdjustment, MaskCombine, MaskComponent, MaskGeometry, RangeMask};
        let mut recipe = EditRecipe::default();
        recipe.masks.push(LocalAdjustment {
            mask: MaskGeometry::Linear { zero_x: 0.1, zero_y: 0.1, full_x: 0.9, full_y: 0.9 },
            components: vec![MaskComponent {
                inverted: false,
                geometry: MaskGeometry::Linear { zero_x: 0.2, zero_y: 0.2, full_x: 0.8, full_y: 0.8 },
                mode: MaskCombine::Add,
            }],
            ..Default::default()
        });
        apply(&mut recipe, Command::Mask(MaskCommand::Rename { index: 0, name: "sky 2".into() }));
        assert_eq!(recipe.masks[0].name.trim(), "sky 2");
        apply(&mut recipe, Command::Mask(MaskCommand::SetComponentMode {
            index: 0, component: 0, mode: MaskCombine::Subtract,
        }));
        assert_eq!(recipe.masks[0].components[0].mode, MaskCombine::Subtract);
        apply(&mut recipe, Command::Mask(MaskCommand::SetComponentInverted {
            index: 0, component: 0, inverted: true,
        }));
        assert!(recipe.masks[0].components[0].inverted);
        apply(&mut recipe, Command::Mask(MaskCommand::SetRange {
            index: 0, range: Some(RangeMask::Luminance { lo_outer: 0.0, lo: 0.0, hi: 1.0, hi_outer: 1.0 }),
        }));
        assert!(matches!(recipe.masks[0].range, Some(RangeMask::Luminance { .. })));
        apply(&mut recipe, Command::Mask(MaskCommand::RemoveComponent { index: 0, component: 0 }));
        assert!(recipe.masks[0].components.is_empty());
        // Missing mask / component arms are no-ops.
        apply(&mut recipe, Command::Mask(MaskCommand::Rename { index: 9, name: "x".into() }));
        apply(&mut recipe, Command::Mask(MaskCommand::RemoveComponent { index: 0, component: 9 }));
        assert_eq!(recipe.masks.len(), 1);
    }

    #[test]
    fn move_and_insert_guard_the_ends_and_the_mask_cap() {
        use autoshade::recipe::LocalAdjustment;
        let mut recipe = EditRecipe::default();
        for name in ["a", "b", "c"] {
            recipe.masks.push(LocalAdjustment { name: name.into(), ..Default::default() });
        }
        let names = |r: &EditRecipe| r.masks.iter().map(|m| m.name.trim().to_string()).collect::<Vec<_>>();
        apply(&mut recipe, Command::Mask(MaskCommand::MoveDown { index: 0 }));
        assert_eq!(names(&recipe), ["b", "a", "c"]);
        apply(&mut recipe, Command::Mask(MaskCommand::MoveUp { index: 2 }));
        assert_eq!(names(&recipe), ["b", "c", "a"]);
        apply(&mut recipe, Command::Mask(MaskCommand::MoveUp { index: 0 }));
        apply(&mut recipe, Command::Mask(MaskCommand::MoveDown { index: 2 }));
        assert_eq!(names(&recipe), ["b", "c", "a"], "the ends do not move");
        apply(
            &mut recipe,
            Command::InsertMask { after: 1, mask: Box::new(LocalAdjustment { name: "copy".into(), ..Default::default() }) },
        );
        assert_eq!(names(&recipe), ["b", "c", "copy", "a"]);
        // The 64-mask cap is the domain's, not only the panel's toast.
        while recipe.masks.len() < 64 {
            recipe.masks.push(Default::default());
        }
        apply(&mut recipe, Command::InsertMask { after: 0, mask: Box::default() });
        assert_eq!(recipe.masks.len(), 64);
    }



}
