//! The open document: the recipe being edited, its variant cards and which one
//! is active. This is what a save persists and what every client (GUI, CLI, web,
//! MCP) means by "the photo's develop".

use autoshade::recipe::EditRecipe;

use crate::model::Variant;

/// The per-photo document. `recipe` is the ACTIVE card's develop; `variants`
/// holds every card (the original negative included); `active` indexes it.
#[derive(Default)]
pub(crate) struct Document {
    pub(crate) recipe: EditRecipe,
    pub(crate) variants: Vec<Variant>,
    pub(crate) active: usize,
}
