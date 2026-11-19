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

/// A size in points, for tests that build screens and frame bodies.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sz {
    pub w: f32,
    pub h: f32,
}

pub(crate) fn sz(w: f32, h: f32) -> Sz {
    Sz { w, h }
}

/// A rectangle reported by the app's test seams (a drawn button's square), as
/// the app's own geometry: tests ask it for the point they click.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RectT {
    pub min: Pt,
    pub max: Pt,
}

pub(crate) fn rect(rect: egui::Rect) -> RectT {
    RectT {
        min: pt(rect.min.x, rect.min.y),
        max: pt(rect.max.x, rect.max.y),
    }
}

impl RectT {
    pub(crate) fn center(self) -> Pt {
        pt((self.min.x + self.max.x) * 0.5, (self.min.y + self.max.y) * 0.5)
    }
}

/// One synthetic input event. Only the events the tests inject are modelled;
/// each carries the app's own geometry, not the framework's.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Event {
    PointerMoved(Pt),
    PointerButton { pos: Pt, pressed: bool },
    Key(Key),
}

/// The keys the GUI tests press, as the app's own names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Escape,
    Enter,
}

impl Key {
    fn into_egui(self) -> egui::Key {
        match self {
            Key::Escape => egui::Key::Escape,
            Key::Enter => egui::Key::Enter,
        }
    }
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
            Event::Key(key) => egui::Event::Key {
                key: key.into_egui(),
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        }
    }
}

/// An in-memory prefs file, so a test can save/restore the way the framework
/// does without naming the framework's storage trait.
#[derive(Default)]
pub(crate) struct PrefsStore(std::collections::HashMap<String, String>);

impl PrefsStore {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    /// The app's prefs, saved the way the framework saves them on exit.
    pub(crate) fn save(app: &mut crate::AutoShadeApp) -> Self {
        let mut store = Self::empty();
        eframe::App::save(app, &mut store);
        store
    }

    /// The raw string stored under the framework's app key.
    pub(crate) fn raw(&self) -> String {
        self.0.get(eframe::APP_KEY).cloned().unwrap_or_default()
    }

    /// Replace the raw stored string (a prefs file from an older version).
    pub(crate) fn set_raw(&mut self, value: impl Into<String>) {
        self.0.insert(eframe::APP_KEY.into(), value.into());
    }

    /// The decoded preferences, if the raw string decodes.
    pub(crate) fn prefs(&self) -> Option<crate::Prefs> {
        eframe::get_value(self, eframe::APP_KEY)
    }
}

impl eframe::Storage for PrefsStore {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.into(), value);
    }

    fn flush(&mut self) {}
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

impl Ctx {
    /// One whole-app frame over `size` at `tick`, injecting `events`.
    pub(crate) fn frame(&mut self, app: &mut crate::AutoShadeApp, size: Sz, tick: usize, events: Vec<Event>) -> Out {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.w, size.h))),
            time: Some(tick as f64 / 2.0),
            events: events.into_iter().map(Event::into_egui).collect(),
            ..Default::default()
        };
        Out(self.0.run(input, |ctx| app.run_frame(ctx)))
    }

    /// A primary click at `at`: press on one frame, release on the next, the
    /// frame counter advancing per frame.
    pub(crate) fn click(&mut self, app: &mut crate::AutoShadeApp, size: Sz, tick: &mut usize, at: Pt) {
        let button = |pressed| Event::PointerButton { pos: at, pressed };
        for events in [vec![Event::PointerMoved(at), button(true)], vec![button(false)]] {
            self.frame(app, size, *tick, events);
            *tick += 1;
        }
    }
}

/// One tall frame of the controls column: a 1400x20 000 screen, every fold
/// open, the left panel at 320 points, `f` drawing inside its scroll area —
/// and the texts it drew.
pub(crate) fn tall_frame(app: &mut crate::AutoShadeApp, f: impl Fn(&mut crate::AutoShadeApp, &mut egui::Ui)) -> Vec<String> {
    let mut ctx = Ctx::new();
    crate::theme::install_theme(&ctx, crate::theme::ThemePref::Dark);
    let out = ctx.run_input(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 20_000.0))),
            ..Default::default()
        },
        |ctx| {
            ctx.memory_mut(|m| m.set_everything_is_visible(true));
            egui::SidePanel::left("controls").default_width(320.0).show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| f(app, ui));
            });
        },
    );
    out.texts()
}

/// One left side panel of the given width, drawn into the current frame.
pub(crate) fn side_panel(ctx: &egui::Context, id: &'static str, width: f32, body: impl FnOnce(&mut egui::Ui)) {
    egui::SidePanel::left(id).default_width(width).show(ctx, |ui| body(ui));
}

impl Ctx {
    /// One frame with the framework's default input but `events` injected.
    pub(crate) fn run_default_events(&mut self, events: Vec<Event>, draw: impl FnMut(&egui::Context)) -> Out {
        self.run_input(
            egui::RawInput {
                events: events.into_iter().map(Event::into_egui).collect(),
                ..Default::default()
            },
            draw,
        )
    }

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
