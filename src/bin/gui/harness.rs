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

    /// One frame over a `w`x`h`-point screen with no clock advance, `draw` as
    /// the frame body.
    pub(crate) fn run(&mut self, w: f32, h: f32, draw: impl FnMut(&egui::Context)) -> Out {
        self.run_with(w, h, None, draw)
    }

    /// One frame over a `w`x`h`-point screen at `tick` (the clock the frame
    /// tests use: half a second per tick), `draw` as the frame body.
    pub(crate) fn run_at(&mut self, w: f32, h: f32, tick: usize, draw: impl FnMut(&egui::Context)) -> Out {
        self.run_with(w, h, Some(tick as f64 / 2.0), draw)
    }

    /// One frame over a `w`x`h`-point screen with an explicit frame clock, for
    /// tests that drive the framework's own animations.
    pub(crate) fn run_with(&mut self, w: f32, h: f32, time: Option<f64>, draw: impl FnMut(&egui::Context)) -> Out {
        self.run_input(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h))),
                time,
                ..Default::default()
            },
            draw,
        )
    }

    fn run_input(&mut self, input: egui::RawInput, draw: impl FnMut(&egui::Context)) -> Out {
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

impl Ctx {
    /// A small example texture, for tests that only need a thumbnail-bearing
    /// card (the framework's own example image).
    pub(crate) fn example_texture(&self, name: &str) -> egui::TextureHandle {
        self.0.load_texture(name, egui::ColorImage::example(), egui::TextureOptions::LINEAR)
    }
}

/// A point on the test screen. Owning the type keeps tests from spelling the
/// framework's geometry on every click.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pt {
    pub x: f32,
    pub y: f32,
}

pub(crate) fn pt(x: f32, y: f32) -> Pt {
    Pt { x, y }
}

/// One synthetic input event. Only the events the tests inject are modelled;
/// each carries the app's own geometry, not the framework's.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Event {
    PointerMoved(Pt),
    PointerButton { pos: Pt, pressed: bool },
}

impl Event {
    fn into_egui(self) -> egui::Event {
        match self {
            Event::PointerMoved(p) => egui::Event::PointerMoved(egui::pos2(p.x, p.y)),
            Event::PointerButton { pos, pressed } => egui::Event::PointerButton {
                pos: egui::pos2(pos.x, pos.y),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        }
    }
}

/// The framework's stock light or dark look, for tests that check what the
/// installed palette does with the framework's defaults.
pub(crate) fn stock_visuals(dark: bool) -> egui::Visuals {
    if dark { egui::Visuals::dark() } else { egui::Visuals::light() }
}

/// The framework's bundled fonts as `(bytes, index)` pairs, for the coverage
/// probe (the app embeds extra symbol faces on top).
pub(crate) fn bundled_fonts() -> Vec<(Vec<u8>, u32)> {
    egui::FontDefinitions::default()
        .font_data
        .values()
        .map(|data| (data.font.to_vec(), data.index))
        .collect()
}

/// One frame whose whole body is a central panel (the canvas-style draws).
pub(crate) fn central_panel(ctx: &egui::Context, body: impl FnOnce(&mut egui::Ui)) {
    egui::CentralPanel::default().show(ctx, |ui| body(ui));
}

/// One left side panel of the given width, drawn into the current frame.
pub(crate) fn side_panel(ctx: &egui::Context, id: &'static str, width: f32, body: impl FnOnce(&mut egui::Ui)) {
    egui::SidePanel::left(id).default_width(width).show(ctx, |ui| body(ui));
}

impl Ctx {
    /// One frame over a `w`x`h`-point screen at `tick`, injecting `events`.
    pub(crate) fn run_events(&mut self, w: f32, h: f32, tick: usize, events: Vec<Event>, draw: impl FnMut(&egui::Context)) -> Out {
        self.run_input(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h))),
                time: Some(tick as f64 / 2.0),
                events: events.into_iter().map(Event::into_egui).collect(),
                ..Default::default()
            },
            draw,
        )
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
