// Round-12 split: body moved verbatim from main.rs's inline
// `mod tests` (indentation kept — raw-string fixtures must not
// change by one byte). `super::*` still resolves to the root.
    use super::*;

    // The tests themselves, in parts under tests/: each file is spliced in here
    // by include!, so its items are items of this module and every test keeps
    // its name and its path. The order is the order the old file had.
    /// Every GUI source file except the tests themselves, concatenated. The
    /// old pins read one named file; splitting a file then turned an invariant
    /// test red even though the invariant held. These scans carry no location
    /// claim, so they read the tree instead.
    fn gui_source() -> &'static str {
        static SOURCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        SOURCE.get_or_init(|| {
            fn walk(dir: &std::path::Path, out: &mut String) {
                let Ok(entries) = std::fs::read_dir(dir) else { return };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        if path.file_name().is_some_and(|n| n == "tests") {
                            continue;
                        }
                        walk(&path, out);
                    } else if path.extension().is_some_and(|e| e == "rs")
                        && let Ok(text) = std::fs::read_to_string(&path) {
                            out.push_str(&text);
                            out.push('\n');
                        }
                }
            }
            let mut out = String::new();
            walk(
                &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/bin/gui"),
                &mut out,
            );
            out
        })
    }

    include!("tests/cards_and_versions.rs");
    include!("tests/canvas_and_delivery.rs");
    include!("tests/theme_fonts_curves.rs");
    include!("tests/develop_and_stash.rs");
    include!("tests/ai_cards.rs");
    include!("tests/strip_and_landing.rs");
    include!("tests/resolution_and_language.rs");
    include!("tests/prefs_and_workers.rs");
    include!("tests/disclosures_and_census.rs");
    include!("tests/panels_and_denoise_prefs.rs");
    include!("tests/sections_and_dots.rs");
    include!("tests/ai_panel.rs");
    include!("tests/turns_and_layout.rs");
    include!("tests/headings_and_widths.rs");
    include!("tests/screenshot.rs");
    include!("tests/lens_blur.rs");
    include!("tests/normal.rs");
    include!("tests/recipes.rs");
    include!("tests/portrait.rs");
    include!("tests/fit_prompt.rs");


    /// One headless frame of the whole app (`run_frame`, i.e. `update`
    /// without the prefs flush) over a `size`-point screen, with `events`.
    fn app_frame(
        ctx: &egui::Context,
        app: &mut AutoShadeApp,
        size: egui::Vec2,
        frame: usize,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(frame as f64 / 2.0),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| app.run_frame(ctx))
    }

    /// A primary click at `at`: press on one frame, release on the next.
    fn click_at(ctx: &egui::Context, app: &mut AutoShadeApp, size: egui::Vec2, frame: &mut usize, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [vec![egui::Event::PointerMoved(at), button(true)], vec![button(false)]] {
            app_frame(ctx, app, size, *frame, events);
            *frame += 1;
        }
    }
