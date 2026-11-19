//! The develop form's renderer: for each control row, call its own `draw`.
//!
//! The registry (`controls.rs`) owns the behaviour: the label resource, the
//! band/reset, the read, the write and the widget. This file owns the walk,
//! nothing else — it does not know a single control by name, and a new control
//! shape is a new `impl ControlWidget`, not a new branch here.
//!
//! A reported change is DATA ([`CapabilityChange`]: id + value); the view
//! applies it through the registry, so the intent path has no function
//! pointers and no variant per control.

use crate::controls::{self, CapabilityChange, ControlWidget};
use crate::*;

/// Draw one registry table into `ui`, in table order, collecting the changes.
/// Returns true when any control changed this frame.
pub(crate) fn render_rows(
    ui: &mut Ui,
    lang: Lang,
    recipe: &autoshade::recipe::EditRecipe,
    table: &[&dyn ControlWidget],
    out: &mut Vec<CapabilityChange>,
) -> bool {
    let mut changed = false;
    for control in table {
        changed |= control.draw(ui, lang, recipe, out);
    }
    changed
}

/// Render a table and apply whatever it reported, in one call for the panels.
pub(crate) fn render_and_apply(
    ui: &mut Ui,
    lang: Lang,
    recipe: &mut autoshade::recipe::EditRecipe,
    table: &[&dyn ControlWidget],
) -> bool {
    let mut changes = Vec::new();
    let changed = render_rows(ui, lang, recipe, table, &mut changes);
    for change in changes {
        controls::apply(recipe, change);
    }
    changed
}
