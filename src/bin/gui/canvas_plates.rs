//! The canvas plates: the Before/After textures, the cross-fade in flight and
//! which photo/version the After plate shows. Presentation state — the pixels
//! live in `PhotoPixels`, the textures are what egui was handed this session.

use crate::view::*;

use std::path::PathBuf;


/// The canvas's uploaded textures and their identity.
pub(crate) struct CanvasPlates {
    pub(crate) before_tex: Option<TextureHandle>,
    pub(crate) after_tex: Option<TextureHandle>,
    /// The cross-fade in flight: the frame being faded OUT (None = fade in from
    /// the empty canvas) and when the new one landed.
    pub(crate) canvas_fade: Option<(Option<TextureHandle>, std::time::Instant)>,
    /// Which photo and version `after_tex` shows, so a landing frame can tell a
    /// slider re-render (no fade) from a swap (fade).
    pub(crate) after_shows: Option<(PathBuf, usize)>,
}

impl Default for CanvasPlates {
    fn default() -> Self {
        Self { before_tex: None, after_tex: None, canvas_fade: None, after_shows: None }
    }
}
