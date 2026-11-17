//! The open photo's pixel state: which source is open and which plates the
//! canvas holds. Separate from `recipe` (an edit, persisted) and from the panels
//! (view state): this is the decoded document the develop renders from.

use std::path::PathBuf;
use std::sync::Arc;

/// The photo and its decoded plates — the state that lives exactly as long as
/// one source is open, and that every panel reads but none owns.
#[derive(Default)]
pub(crate) struct PhotoPixels {
    /// The path whose pixels are open (None = no photo).
    pub(crate) src_path: Option<PathBuf>,
    /// The ACTIVE variant's base pixels — shared with the preview worker by Arc.
    pub(crate) base_preview: Option<Arc<image::DynamicImage>>,
    /// The pristine source neutral (RAW develop / loaded image), decoded once
    /// per open. Source-based variants share this same allocation.
    pub(crate) source_preview: Option<Arc<image::DynamicImage>>,
    /// How many clockwise quarter turns are currently BAKED INTO the plates
    /// (R27): a turn is applied to the pixels, so this records the pixel frame.
    pub(crate) base_turns: u8,
    /// The active pixel source's full-resolution SHORT EDGE, cached against the
    /// path and its modification time — the film the Detail radii are in.
    pub(crate) film_edge: Option<(PathBuf, Option<std::time::SystemTime>, Option<u32>)>,
}
