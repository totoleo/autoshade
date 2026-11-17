//! The editing session: what one open document remembers between frames that
//! is NOT persisted — the undo/redo history, the last committed step, the
//! saved-recipe baseline and the dirty flag. It outlives panel redraws and dies
//! when another source opens.

use crate::model::HistoryState;

/// The per-photo editing session. `hist` is the history model the undo/redo
/// verbs write; `dirty` says the recipe moved since the last develop was
/// dispatched (the preview scheduler's input).
#[derive(Default)]
pub(crate) struct EditorSession {
    pub(crate) hist: HistoryState,
    pub(crate) dirty: bool,
}
