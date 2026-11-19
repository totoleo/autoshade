#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_names)]
//! Test-support half of the anti-corruption layer.
//!
//! The GUI tests drive the real framework: they build raw input, run frames and
//! rasterise what was drawn. That is legitimate, but it is the ONE kind of code
//! allowed to name the framework besides `view.rs` — so it lives here, behind
//! the same `clippy.toml` allow, and the test files call these helpers instead
//! of the framework's names.
//!
//! The module grows file by file; the test files shrink until `mod tests` itself
//! can drop its allow.

use eframe::egui;

/// The headless frame context a test drives, as the app's own type. It owns the
/// framework context so a test can hand it to code that expects one through
/// `Deref`, and so no test file has to spell the framework's name.
pub(crate) struct Ctx(egui::Context);

impl Ctx {
    pub(crate) fn new() -> Self {
        Self(egui::Context::default())
    }

    /// One frame over a `w`x`h`-point screen, with `draw` as the frame body.
    pub(crate) fn run(&mut self, w: f32, h: f32, tick: usize, draw: impl FnMut(&egui::Context)) -> Out {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h))),
            time: Some(tick as f64 / 2.0),
            ..Default::default()
        };
        Out(self.0.run(input, draw))
    }

    /// One frame with the framework's default input (no screen rect), with
    /// `draw` as the frame body.
    pub(crate) fn run_default(&mut self, draw: impl FnMut(&egui::Context)) -> Out {
        Out(self.0.run(egui::RawInput::default(), draw))
    }

    /// One frame whose whole body is a central panel (the canvas-style draws).
    pub(crate) fn central(&mut self, body: impl FnOnce(&mut egui::Ui)) {
        let mut body = Some(body);
        self.run_default(move |ctx| {
            if let Some(body) = body.take() {
                egui::CentralPanel::default().show(ctx, |ui| body(ui));
            }
        });
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

/// What one frame produced, as far as a test asks: the drawn texts (and, in
/// later slices, the shapes and the picture).
pub(crate) struct Out(egui::FullOutput);

impl Out {
    /// Every text the frame drew, in paint order. Framework shapes are walked
    /// here so no test file has to.
    pub(crate) fn texts(&self) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        self.0.shapes.iter().for_each(|clipped| walk(&clipped.shape, &mut out));
        out
    }
}
