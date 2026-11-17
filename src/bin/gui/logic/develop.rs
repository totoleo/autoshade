//! The develop panel's mask-list and version-list verbs, as actions the view
//! can ask for. The panel draws and binds sliders; moving/duplicating a mask
//! and loading/deleting a version are the app's business.

use std::path::PathBuf;

/// What the develop panel asks for beyond its sliders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DevelopAction {
    /// Run the Retouch panel's AI denoise from the noise switch.
    AiDenoise,
    /// Stack these extra frames into a new ▦ card.
    Stack(Vec<PathBuf>),
    /// Start the raster brush on this mask (None: a fresh bitmap mask).
    MaskBrush(Option<usize>),
    /// Segment a subject/sky target and land it as a mask.
    Segment { target: &'static str, label: &'static str },
    /// Refine this mask's raster to full resolution.
    MaskRefine(usize),
    /// The mask list is render order: move one row up/down.
    MoveMaskUp(usize),
    MoveMaskDown(usize),
    /// Copy a mask (bitmap rasters detached, so the twins never share a file).
    DuplicateMask(usize),
    /// Replace the parameters with this snapshot (one Ctrl+Z undoes).
    LoadVersion(u32),
    /// Delete this snapshot and its frozen rasters.
    DeleteVersion(u32),
    /// Snapshot the live canvas as the next numbered version.
    SaveVersion,
    /// Commit the version row's typed name to the store.
    CommitVersionName,
}
