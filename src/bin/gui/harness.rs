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

    pub(crate) fn right(self) -> f32 {
        self.max.x
    }

    pub(crate) fn width(self) -> f32 {
        self.max.x - self.min.x
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

// ---- CPU screenshots ------------------------------------------------------
//
// The tests look at the window as a PNG without opening one. The tessellated
// framework output is rasterised here, in the harness, so a test only names
// the picture and the rects.

/// Every texture the framework handed out so far, as premultiplied sRGBA rows.
#[derive(Default)]
pub(crate) struct ShotTextures(std::collections::HashMap<egui::TextureId, (usize, usize, Vec<egui::Color32>)>);

impl ShotTextures {
    pub(crate) fn apply(&mut self, delta: &egui::TexturesDelta) {
        for (id, d) in &delta.set {
            let (w, h) = (d.image.width(), d.image.height());
            let px: Vec<egui::Color32> = match &d.image {
                egui::ImageData::Color(c) => c.pixels.clone(),
            };
            match d.pos {
                None => {
                    self.0.insert(*id, (w, h, px));
                }
                Some([x0, y0]) => {
                    let (tw, _, tex) = self.0.get_mut(id).expect("patch onto a known texture");
                    for y in 0..h {
                        for x in 0..w {
                            tex[(y0 + y) * *tw + x0 + x] = px[y * w + x];
                        }
                    }
                }
            }
        }
    }
}

/// Draw `prims` over a `bg`-filled canvas of `size` points at `ppp`, the way the
/// framework's own GL backend blends: premultiplied colour x texture, source-over
/// in gamma space, scissored to each primitive's clip rect.
pub(crate) fn rasterise(
    prims: &[egui::ClippedPrimitive],
    tex: &ShotTextures,
    size: egui::Vec2,
    ppp: f32,
    bg: egui::Color32,
) -> image::RgbaImage {
    let (w, h) = ((size.x * ppp) as usize, (size.y * ppp) as usize);
    let mut buf = vec![[bg.r() as f32, bg.g() as f32, bg.b() as f32]; w * h];
    for p in prims {
        let egui::epaint::Primitive::Mesh(mesh) = &p.primitive else { continue };
        let Some((tw, th, texels)) = tex.0.get(&mesh.texture_id) else { continue };
        let clip = p.clip_rect;
        let (cx0, cy0) = ((clip.min.x * ppp).max(0.0), (clip.min.y * ppp).max(0.0));
        let (cx1, cy1) = ((clip.max.x * ppp).ceil().min(w as f32), (clip.max.y * ppp).ceil().min(h as f32));
        for t in mesh.indices.chunks_exact(3) {
            let v = [&mesh.vertices[t[0] as usize], &mesh.vertices[t[1] as usize], &mesh.vertices[t[2] as usize]];
            let pt = |i: usize| (v[i].pos.x * ppp, v[i].pos.y * ppp);
            let (a, b, c) = (pt(0), pt(1), pt(2));
            let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            if area.abs() < 1e-6 {
                continue;
            }
            let x0 = a.0.min(b.0).min(c.0).floor().max(cx0) as usize;
            let x1 = a.0.max(b.0).max(c.0).ceil().min(cx1).max(0.0) as usize;
            let y0 = a.1.min(b.1).min(c.1).floor().max(cy0) as usize;
            let y1 = a.1.max(b.1).max(c.1).ceil().min(cy1).max(0.0) as usize;
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((b.0 - px) * (c.1 - py) - (b.1 - py) * (c.0 - px)) / area;
                    let w1 = ((c.0 - px) * (a.1 - py) - (c.1 - py) * (a.0 - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let mix = |f: fn(&egui::epaint::Vertex) -> f32| w0 * f(v[0]) + w1 * f(v[1]) + w2 * f(v[2]);
                    let tx = ((mix(|q| q.uv.x) * *tw as f32) as usize).min(tw - 1);
                    let ty = ((mix(|q| q.uv.y) * *th as f32) as usize).min(th - 1);
                    let texel = texels[ty * tw + tx].to_array();
                    let colour = [
                        mix(|q| q.color.r() as f32),
                        mix(|q| q.color.g() as f32),
                        mix(|q| q.color.b() as f32),
                        mix(|q| q.color.a() as f32),
                    ];
                    let src: [f32; 4] = std::array::from_fn(|k| colour[k] * texel[k] as f32 / 255.0);
                    let dst = &mut buf[y * w + x];
                    for k in 0..3 {
                        dst[k] = src[k] + dst[k] * (1.0 - src[3] / 255.0);
                    }
                }
            }
        }
    }
    image::RgbaImage::from_fn(w as u32, h as u32, |x, y| {
        let p = buf[y as usize * w + x as usize];
        image::Rgba([p[0].round() as u8, p[1].round() as u8, p[2].round() as u8, 255])
    })
}

/// The controls panel as the app lays it (`controls_side_panel` around
/// `controls_column`), as if the user had dragged it to `width` px; its rect.
pub(crate) fn controls_panel(ctx: &egui::Context, app: &mut crate::AutoShadeApp, width: f32) -> RectT {
    let id = egui::Id::new("controls");
    if egui::containers::panel::PanelState::load(ctx, id).is_none() {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 100.0));
        ctx.data_mut(|d| d.insert_persisted(id, egui::containers::panel::PanelState { rect }));
    }
    rect(crate::side_panel::controls_side_panel(ctx, |ui| app.controls_column(ui)).response.rect)
}

/// [`controls_panel`] run `frames` times over a `size`-point screen at `ppp`;
/// the last frame's rasterised picture, output and the panel's rect.
pub(crate) fn controls_shot(
    app: &mut crate::AutoShadeApp,
    theme: crate::theme::ThemePref,
    size: Sz,
    panel_w: f32,
    ppp: f32,
    frames: usize,
) -> (image::RgbaImage, Out, RectT) {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    crate::theme::install_theme(&ctx, theme);
    let mut tex = ShotTextures::default();
    let mut last = None;
    let mut panel = rect(egui::Rect::NOTHING);
    for i in 0..frames {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.w, size.h))),
            time: Some(i as f64 / 60.0),
            ..Default::default()
        };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
        let out = ctx.run(input, |ctx| {
            ctx.memory_mut(|m| m.set_everything_is_visible(true));
            panel = controls_panel(ctx, app, panel_w);
        });
        tex.apply(&out.textures_delta);
        last = Some(out);
    }
    let out = last.expect("at least one frame");
    // The panel's own layer only: the show-everything hook also pops every
    // tooltip, which the picture is not about.
    let shapes: Vec<_> = out.shapes.iter().filter(|c| c.clip_rect.right() <= panel.max.x + 1.0).cloned().collect();
    let prims = ctx.tessellate(shapes, out.pixels_per_point);
    let bg = ctx.style().visuals.panel_fill;
    (rasterise(&prims, &tex, egui::vec2(size.w, size.h), out.pixels_per_point, bg), Out(out), panel)
}

/// What the controls shot says about the scrollbar band and the content edge:
/// texts reaching into the bar's band, and shapes drawn past the content edge.
pub(crate) struct GutterReport {
    pub past: Vec<String>,
    pub wide: Vec<String>,
    pub bar_left: f32,
    pub content_right: f32,
}

pub(crate) fn controls_gutter_report(shot: &Out, panel: RectT, theme: crate::theme::ThemePref) -> GutterReport {
    fn walk(shape: &egui::Shape, clip: egui::Rect, bar_left: f32, past: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => {
                let r = t.visual_bounding_rect().intersect(clip);
                if r.is_positive() && r.right() > bar_left + 0.5 && !t.galley.text().trim().is_empty() {
                    let head: String = t.galley.text().chars().take(40).collect();
                    past.push(format!("{head:?} right {:.1}", r.right()));
                }
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, clip, bar_left, past)),
            _ => {}
        }
    }
    let ctx = egui::Context::default();
    crate::theme::install_theme(&ctx, theme);
    let style = ctx.style();
    // The panel frame's own right margin, then the bar inside it.
    let content_right = panel.max.x - f32::from(egui::Frame::side_top_panel(&style).inner_margin.right);
    let bar_left = content_right - style.spacing.scroll.bar_width;
    let mut past = Vec::new();
    let mut wide = Vec::new();
    for c in shot.0.shapes.iter().filter(|c| c.clip_rect.right() <= panel.max.x + 0.5) {
        walk(&c.shape, c.clip_rect, bar_left, &mut past);
        // What reaches the screen: the shape cut by its own clip (a prompt's
        // hint runs on inside its field's clip), and not the panel's own frame
        // fill, which spans the whole panel.
        let r = c.shape.visual_bounding_rect().intersect(c.clip_rect);
        if r.is_positive() && r.left() > panel.min.x + 0.5 && r.right() > content_right + 0.5 {
            wide.push(format!("{r:?}"));
        }
    }
    GutterReport { past, wide, bar_left, content_right }
}

/// The analysis full-text window alone, on a 720 x 560 screen at 2x.
pub(crate) fn window_shot(app: &mut crate::AutoShadeApp, theme: crate::theme::ThemePref) -> image::RgbaImage {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    crate::theme::install_theme(&ctx, theme);
    let size = egui::vec2(720.0, 560.0);
    let mut tex = ShotTextures::default();
    let mut last = None;
    for i in 0..6 {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(i as f64 / 2.0),
            ..Default::default()
        };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(2.0);
        let out = ctx.run(input, |ctx| app.commands().analysis_full_window(ctx));
        tex.apply(&out.textures_delta);
        last = Some(out);
    }
    let out = last.expect("frames ran");
    let prims = ctx.tessellate(out.shapes.clone(), out.pixels_per_point);
    rasterise(&prims, &tex, size, out.pixels_per_point, theme.colors().canvas_bed)
}

/// The whole window as the app lays it over a synthetic photo, at `size` points
/// and 1.5x: toolbar, status bar, variant strip, side panels and canvas.
pub(crate) fn full_window_shot(app: &mut crate::AutoShadeApp, theme: crate::theme::ThemePref, size: Sz) -> image::RgbaImage {
    // The app reads its own theme field for the bars and the canvas bed.
    app.workspace_mut().ui_mut().theme = theme;
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    crate::theme::install_theme(&ctx, theme);
    // A warm-rock / blue-sky gradient standing in for a photo.
    let (w, h) = (600usize, 400usize);
    let px: Vec<egui::Color32> = (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as f32 / w as f32, (i / w) as f32 / h as f32);
            if y < 0.45 {
                egui::Color32::from_rgb((70.0 + 60.0 * y) as u8, (120.0 + 50.0 * y) as u8, (200.0 - 30.0 * x) as u8)
            } else {
                egui::Color32::from_rgb((170.0 + 50.0 * x) as u8, (80.0 + 30.0 * y) as u8, (45.0 + 20.0 * x) as u8)
            }
        })
        .collect();
    let photo = egui::ColorImage::new([w, h], px);
    app.editor_mut().plates_mut().after_tex = Some(ctx.load_texture("shot-photo", photo, egui::TextureOptions::LINEAR));
    let ppp = 1.5;
    let mut tex = ShotTextures::default();
    let mut last = None;
    for i in 0..6 {
        // The pointer rests on the canvas, so an armed brush shows its ring.
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.w, size.h))),
            time: Some(i as f64 / 2.0),
            events: vec![egui::Event::PointerMoved(egui::pos2(size.w * 0.7, size.h * 0.3))],
            ..Default::default()
        };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
        let out = ctx.run(input, |ctx| {
            app.upd_top_bar(ctx);
            app.upd_status_bar(ctx);
            app.upd_strips_and_side_panels(ctx);
            app.upd_central(ctx);
            // Sheets (Settings, keyboard, the recipe library) are part of the
            // window too; with none open this draws nothing.
            app.upd_sheets(ctx);
        });
        tex.apply(&out.textures_delta);
        last = Some(out);
    }
    let out = last.expect("frames ran");
    let prims = ctx.tessellate(out.shapes.clone(), out.pixels_per_point);
    rasterise(&prims, &tex, egui::vec2(size.w, size.h), out.pixels_per_point, egui::Color32::BLACK)
}
