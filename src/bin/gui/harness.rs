#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_names)]
//! Test-support half of the anti-corruption layer.
//!
//! The GUI tests drive the real framework: they build raw input, run frames and
//! rasterise what was drawn. That is legitimate, but it is the ONE kind of code
//! allowed to name the framework besides `view.rs` — so it lives here, behind
//! the same `clippy.toml` allow, and the test files call these helpers instead
//! of the framework's names.
//!
//! Migrated so far: the frame context (`Ctx`). The rest of the harness (events,
//! geometry, screenshots, storage) moves here file by file; the module grows and
//! the test files shrink until `mod tests` itself can drop its allow.

use eframe::egui;

/// The headless frame context a test drives, as the app's own type. It owns the
/// framework context so a test can hand it to code that expects one through
/// `Deref`, and so no test file has to spell the framework's name.
pub(crate) struct Ctx(egui::Context);

impl Ctx {
    pub(crate) fn new() -> Self {
        Self(egui::Context::default())
    }
}

impl Default for Ctx {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for Ctx {
    type Target = egui::Context;

    fn deref(&self) -> &egui::Context {
        &self.0
    }
}
