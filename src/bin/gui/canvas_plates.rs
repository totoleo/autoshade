//! The canvas plates: the Before/After textures, the cross-fade in flight and
//! which photo/version the After plate shows. Presentation state — the pixels
//! live in `PhotoPixels`, the textures are what egui was handed this session.

use std::path::PathBuf;

use egui::TextureHandle;

/// The canvas's uploaded textures and their identity.
#[derive(Default)]
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
