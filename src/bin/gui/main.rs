// Release builds run WITHOUT a console window — a GUI app shouldn't flash a
// terminal on launch. Debug keeps the console so panics/logs stay visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// The framework quarantine is a clippy rule (see clippy.toml): only the door
// and the harness may name the framework shell or the types it owns.
#![warn(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_names)]

//! AutoShade — native desktop GUI (egui/eframe).
//!
//! A real native window (no localhost server, no webview): it links the
//! `autoshade` engine library and calls `decode` / `render` / `pipeline` directly
//! in-process. Open a RAW or image, develop it with live before/after, run the
//! AI auto-develop, and export — all from one window.
//!
//! Build/run: `cargo run --release --features gui --bin autoshade-gui`

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

// NOTE: `MaskRole` is addressed only by method here (`m.role.en_name()` in the
// mask row), never named as a type, so it is intentionally NOT imported — the
// enum lives in recipe.rs and is set by the engine, not constructed in the GUI.
use autoshade::recipe::{ColorGrade, CurvePoint, EditRecipe, Hsl, MaskGeometry, RangeMask};
use image::GenericImageView;

// Native-GUI text: the resources live in the library (`autoshade::text`), the
// GUI binds them with the platform's modifier hint and re-exports the key
// constants. See i18n.rs.
mod view;
mod i18n;
mod landing_analysis;
mod landing_canvas;
mod landing_worker;
mod landing_persist;
use i18n::*;
use view::*;

mod model;
mod services;
mod commands;
pub(crate) use commands::Commands;
mod analysis_session;
mod canvas_plates;
mod document;
mod editor;
mod workspace;
mod photo;
mod scheduler;
mod session;
mod persist;
mod sections;
mod controls;
// The PORTABLE half of the quit guard: its state machine, and the
// process-wide slots the delegate method reads through. Compiled and tested on
// every platform on purpose — the Windows battery is what gates this decision
// — but its non-test caller is `macos.rs`, so off macOS the entry points are
// genuinely uncalled and the compiler is right to say so. The allow is scoped
// to exactly that case: on macOS real dead code here is still caught.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod quit;
// The Objective-C half of the quit guard. `quit.rs` above is its portable
// state machine and is compiled (and tested) on every platform.
#[cfg(target_os = "macos")]
mod macos;
mod buttons;
mod shortcuts;
mod ui_kit;
mod forms;
mod shell;
mod headings;
mod theme;
mod util;
mod history_actions;
mod open_actions;
mod variant_actions;
mod version_actions;
mod actions;
mod logic;
mod app;
mod budget;
mod ai_start_actions;
mod canvas;
mod canvas_masters;
mod export;
mod masks;
mod normal_actions;
mod panel_view;
mod sliders;
mod panels;
mod portrait_actions;
mod recipe_actions;
mod retouch_actions;
mod runtime;
mod settings_actions;
mod side_panel;
mod window_chrome;
use model::*;
use services::*;
use analysis_session::*;
use canvas_plates::*;
use document::*;
use editor::*;
use workspace::*;
use photo::*;
use scheduler::*;
use session::*;
use persist::*;
use buttons::*;
use shortcuts::*;
use ui_kit::*;
use forms::*;
use headings::*;
use theme::*;
use util::*;
use app::*;
use panel_view::*;
use sliders::*;
use panels::ai::fit_explainer;
use version_actions::version_is_from;

use side_panel::*;



/// See main(): the last-crash report lands at `<store root>/panic.log`,
/// and (Windows) a native message box points the user at it.
fn install_panic_reporter() {
    let default = std::panic::take_hook();
    // Captured HERE, where this is provably the main thread: `main()` calls
    // this before anything else. The macOS dialog below is main-thread-only.
    #[cfg(target_os = "macos")]
    let main_thread = std::thread::current().id();
    std::panic::set_hook(Box::new(move |info| {
        // CONTAINED or FATAL — the same rule about claims matching outcomes,
        // applied to the headline itself. `decode::guard_parser_panic` and
        // `spawn_worker` both catch, and this hook runs INSIDE both, so one
        // unreadable photo used to raise "must close" over an app that kept
        // running and then reported that file's own error. The report is
        // written either way — a contained panic is still a defect worth
        // recording — but only the fatal half gets a modal.
        let contained = autoshade::panic_guard::panic_is_recoverable();
        // The claim must match the outcome (review R12-10): on a first
        // launch the store root may not exist yet, and a failed write must
        // not direct the user at a report that is not there.
        let root = autoshade::store::store_root();
        let _ = std::fs::create_dir_all(&root);
        let log = root.join("panic.log");
        let headline = if contained {
            "AutoShade recovered from an internal error"
        } else {
            "AutoShade crashed"
        };
        let wrote = std::fs::write(&log, format!("{headline}: {info}\n").as_bytes()).is_ok();
        if contained {
            // The failure still reaches the user — as the operation's own
            // error line, from the `Msg` the guard synthesizes — and that
            // line names the FILE, which a modal about the process cannot.
            default(info);
            return;
        }
        // Built once, shown by whichever dialog this platform has. The claim
        // still has to match the outcome, which is what `wrote` decides.
        let msg = if wrote {
            format!(
                "AutoShade hit an internal error and must close.\n\n{info}\n\nA report was written to:\n{}",
                log.display()
            )
        } else {
            format!(
                "AutoShade hit an internal error and must close.\n\n{info}\n\n(a report could NOT be written to {})",
                log.display()
            )
        };
        #[cfg(windows)]
        message_box(&msg);
        // macOS has exactly the problem Windows has — a release build is
        // windowed, so the panic's stderr reaches nobody and the window simply
        // vanishes — and rfd is already in the tree for the file pickers, so
        // this costs no new dependency.
        //
        // MAIN THREAD ONLY. rfd's macOS backend dispatches onto the main queue
        // and blocks for the answer, so raising it from a worker while the main
        // thread is itself unwinding would hang a process that was merely
        // crashing. A worker panic still writes `panic.log` and still reaches
        // the default hook — it just does not get a dialog.
        #[cfg(target_os = "macos")]
        if std::thread::current().id() == main_thread {
            let _ = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("AutoShade")
                .set_description(&msg)
                .show();
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        let _ = &msg;
        default(info);
    }));
}

/// Native blocking message box (user32, the store.rs raw-link pattern —
/// windows-sys is in the tree but without the WindowsAndMessaging gate).
#[cfg(windows)]
fn message_box(text: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        #[link_name = "MessageBoxW"]
        fn message_box_w(hwnd: usize, text: *const u16, caption: *const u16, utype: u32) -> i32;
    }
    const MB_OK_ICONERROR: u32 = 0x0000_0010;
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let title: Vec<u16> = "AutoShade".encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: NUL-terminated buffers, live across the synchronous call.
    unsafe { message_box_w(0, wide.as_ptr(), title.as_ptr(), MB_OK_ICONERROR) };
}

fn main() {
    // A windowed release build has NO console: a panic wrote its report to a
    // stderr nobody can see and the window simply vanished (L14-7). The hook
    // writes the report beside the develop store and says so in a native
    // message box — the one channel that needs no working event loop.
    install_panic_reporter();
    // Before the first render can touch rayon: on an already-tight machine
    // the global pool is built narrower than one-per-logical-core (measured
    // trade-off in budget.rs).
    budget::clamp_global_rayon();
    if let Err(why) = view::run() {
        eprintln!("AutoShade could not start: {why}");
        std::process::exit(1);
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_names)]
mod tests;

