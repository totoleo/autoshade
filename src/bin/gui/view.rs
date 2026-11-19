#![allow(clippy::disallowed_types, clippy::disallowed_methods, clippy::disallowed_names)]

//! The one door to the UI framework.
//!
//! Production code speaks the app's vocabulary: it imports the framework's
//! names from this module (unqualified, via the crate prelude) and never spells
//! `egui::` itself. Tests still drive the framework directly through the
//! `egui` alias re-exported here. A framework change starts in this module.
//!
//! This first layer isolates the DEPENDENCY and the NAME. The types are still
//! framework aliases: the deeper layer (app-owned value types and a paint
//! vocabulary with conversions) is a separate, larger decision.

use std::path::Path;

/// The app's event-loop context, as the app's own type. Production code names
/// this, never the framework's `Context`: the framework's type appears only
/// here, in the anti-corruption layer. It owns a clone (the framework shares
/// its context), so `Deref` still serves the methods the app calls, and the
/// entries below convert at the boundary.
#[derive(Clone)]
pub(crate) struct Ctx(pub(crate) egui::Context);

impl Ctx {
    /// A context from the framework's own handle (a cheap clone).
    pub(crate) fn from_ref(ctx: &egui::Context) -> Self {
        Self(ctx.clone())
    }

    /// A headless context: the services' default and the tests.
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

use crate::theme::{install_fonts, install_theme, ThemePref};
use crate::{AutoShadeApp, Prefs};

pub(crate) use eframe::egui::{
    Align,
    Align2,
    Area,
    Button,
    CentralPanel,
    Checkbox,
    CollapsingHeader,
    CollapsingResponse,
    Color32,
    ColorImage,
    ComboBox,
    Context,
    CornerRadius,
    CursorIcon,
    DragValue,
    Event,
    FontData,
    FontDefinitions,
    FontFamily,
    FontId,
    Frame,
    Grid,
    IconData,
    Id,
    Image,
    InnerResponse,
    InputState,
    Key,
    Label,
    LayerId,
    Layout,
    text::LayoutJob,
    Margin,
    Mesh,
    Modifiers,
    Order,
    PointerButton,
    Pos2,
    ProgressBar,
    Rangef,
    Rect,
    epaint::RectShape,
    Response,
    RichText,
    ScrollArea,
    Sense,
    epaint::Shadow,
    Shape,
    SidePanel,
    Slider,
    Stroke,
    StrokeKind,
    Style,
    SystemTheme,
    TextEdit,
    TextFormat,
    TextStyle,
    TextWrapMode,
    TextureHandle,
    TextureId,
    TextureOptions,
    ThemePreference,
    TopBottomPanel,
    Ui,
    Vec2,
    ViewportBuilder,
    ViewportCommand,
    Visuals,
    WidgetText,
    Window,
    collapsing_header,
    ecolor,
    emath,
    lerp,
    load,
    containers::panel,
    pos2,
    vec2,
    viewport,
};

/// The eframe STORAGE KEY — not a display name; the window title is set on the
/// viewport. With `persistence_path` unset (every real launch) eframe derives
/// the prefs file from THIS string: `%APPDATA%\<key>\data\app.ron` on Windows
/// (eframe 0.29 glow_integration.rs:200 -> file_storage.rs:37).
///
/// One spelling everywhere since the C2 ruling (2026-08-31: migrate, do not
/// accept the old name). Renaming the key ALONE would silently abandon every
/// existing user's window geometry, last library, view mode, export options
/// and theme — so [`adopt_pre_rename_prefs`] moves the pre-rename directory
/// first, on the develop store's own rename-or-fall-back doctrine
/// (`store::adopt_pre_rename_root`).
pub(crate) const STORAGE_KEY: &str = "AutoShade";
/// The spelling every pre-rename install wrote its prefs under; read (and
/// renamed away) by [`adopt_pre_rename_prefs`], never written again.
pub(crate) const LEGACY_STORAGE_KEY: &str = "Autoshop";
/// macOS opts out for `store::ADOPT_PRE_RENAME`'s reason: `Application
/// Support` holds every program's folders under DISPLAY names, no Mac ever ran
/// the pre-rename spelling, and on case-insensitive APFS an adoption could
/// only rename a stranger's directory.
pub(crate) const ADOPT_PRE_RENAME_PREFS: bool = !cfg!(target_os = "macos");

/// Outcome of [`adopt_prefs_between`] — `store::RootAdoption`'s shape, for the
/// prefs directory.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PrefsAdoption {
    /// No pre-rename directory (or the platform opts out): nothing to move.
    Nothing,
    /// Both spellings exist: the new one wins and the old is not touched.
    KeptBoth,
    /// The pre-rename directory was renamed to the new spelling, prefs inside.
    Migrated,
    /// The rename failed. The caller keeps the LEGACY key for this session —
    /// the prefs keep working, nothing is reset — and the next launch retries.
    FellBack,
}

/// The migration core, on explicit paths so the battery drives it with temp
/// directories: rename `legacy` to `current` when only `legacy` exists,
/// making `current`'s parent first because on Windows it is not there yet.
pub(crate) fn adopt_prefs_between(current: &Path, legacy: &Path, adopt: bool) -> PrefsAdoption {
    if !adopt || !legacy.is_dir() {
        return PrefsAdoption::Nothing;
    }
    if current.exists() {
        return PrefsAdoption::KeptBoth;
    }
    // `eframe::storage_dir` is TWO levels deep on Windows —
    // `%APPDATA%\<key>\data` — so `current`'s parent is the app's own folder
    // under the NEW spelling, which a machine that never ran the new name does
    // not have. Renaming into it fails ERROR_PATH_NOT_FOUND and the retry the
    // FellBack arm promises fails identically on every later launch. The two
    // sibling adoptions (`store::adopt_pre_rename_root`,
    // `serve::adopted_export_registry_root`) rename WITHIN a directory that
    // already exists, which is why only this one needs the parent made first.
    if let Some(parent) = current.parent()
        && !parent.as_os_str().is_empty()
        && std::fs::create_dir_all(parent).is_err()
    {
        return PrefsAdoption::FellBack;
    }
    match std::fs::rename(legacy, current) {
        Ok(()) => PrefsAdoption::Migrated,
        Err(_) => PrefsAdoption::FellBack,
    }
}

/// Resolve the storage key for THIS launch, adopting the pre-rename prefs
/// directory on the way. Sandboxed runs (`AUTOSHADE_DATA_DIR`) never touch the
/// real per-user directories — `persistence_path` overrides storage wholesale,
/// so there is nothing to migrate and nothing real to disturb.
fn adopt_pre_rename_prefs() -> &'static str {
    if autoshade::config::live_env_os("AUTOSHADE_DATA_DIR").is_some() {
        return STORAGE_KEY;
    }
    let (Some(current), Some(legacy)) =
        (eframe::storage_dir(STORAGE_KEY), eframe::storage_dir(LEGACY_STORAGE_KEY))
    else {
        return STORAGE_KEY;
    };
    match adopt_prefs_between(&current, &legacy, ADOPT_PRE_RENAME_PREFS) {
        PrefsAdoption::Nothing => STORAGE_KEY,
        PrefsAdoption::Migrated => {
            eprintln!(
                "note: your window/library preferences moved from {LEGACY_STORAGE_KEY} to \
                 {STORAGE_KEY} (the app was renamed). Nothing was copied or duplicated."
            );
            STORAGE_KEY
        }
        PrefsAdoption::KeptBoth => {
            eprintln!(
                "note: a pre-rename {LEGACY_STORAGE_KEY} preferences folder sits beside the \
                 {STORAGE_KEY} one this version uses. Nothing was moved or merged — \
                 {STORAGE_KEY} is in use."
            );
            STORAGE_KEY
        }
        PrefsAdoption::FellBack => {
            eprintln!(
                "warning: could not move your preferences from {LEGACY_STORAGE_KEY} to \
                 {STORAGE_KEY}. Still using the old folder, so nothing is lost; the next \
                 launch tries again."
            );
            LEGACY_STORAGE_KEY
        }
    }
}

/// Decode the embedded AutoShade icon for the window title bar / taskbar.
fn app_icon() -> IconData {
    let img = image::load_from_memory(include_bytes!("../../../assets/icon_256.png"))
        .expect("embedded icon decodes")
        .to_rgba8();
    let (width, height) = img.dimensions();
    IconData { rgba: img.into_raw(), width, height }
}

pub(crate) fn run() -> eframe::Result<()> {
    // A windowed release build has NO console: a panic wrote its report to a
    // stderr nobody can see and the window simply vanished (L14-7). The hook
    // writes the report beside the develop store and says so in a native
    // message box — the one channel that needs no working event loop.
    let opts = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([1400.0, 880.0])
            // Below this the wrapped toolbar rows + two side panels leave no
            // usable canvas; wrapping (not clipping) covers everything above.
            .with_min_inner_size([980.0, 620.0])
            .with_title("AutoShade")
            // Windows: the toolbar is the title bar (`window_chrome.rs`).
            .with_decorations(!crate::window_chrome::CUSTOM_CHROME)
            .with_icon(std::sync::Arc::new(app_icon())),
        // The develop STORE already follows AUTOSHADE_DATA_DIR; the eframe
        // prefs file (last library, theme, window geometry) did not, so a
        // sandboxed E2E launch read — and on exit rewrote — the REAL user's
        // prefs, and its window opened onto their actual photo library. One
        // sandbox variable must mean the whole app is sandboxed.
        persistence_path: autoshade::config::live_env_os("AUTOSHADE_DATA_DIR")
            .map(|d| std::path::PathBuf::from(d).join("gui-prefs.ron")),
        ..Default::default()
    };
    eframe::run_native(
        // See [`STORAGE_KEY`]: a storage key, not a display name. The
        // adoption resolves which spelling answers THIS launch — the new one,
        // unless a failed migration keeps the legacy directory in use.
        adopt_pre_rename_prefs(),
        opts,
        Box::new(|cc| {
            let ctx = Ctx::from_ref(&cc.egui_ctx);
            install_fonts(&ctx); // embedded symbol subsets + system CJK
            // Dark before prefs are readable; AutoShadeApp::new re-installs the
            // saved choice one call later (same shape as the greeting string).
            install_theme(&ctx, ThemePref::Dark);
            let prefs = cc.storage.and_then(|s| eframe::get_value::<Prefs>(s, eframe::APP_KEY));
            Ok(Box::new(AutoShadeApp::new(&ctx, prefs))) // restores prefs + last library
        }),
    )
}


/// The framework's application hooks: one frame, and the prefs flush. The body
/// of a frame lives in `shell.rs`; what persists belongs to `app.rs`.
impl eframe::App for AutoShadeApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.run_frame(&Ctx::from_ref(ctx));
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.prefs_snapshot());
    }
}
