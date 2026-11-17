//! GUI data model: view-state types, messages, prefs, constants.

use super::*;

/// How the preview area is laid out. `AfterOnly` gives the edit the whole
/// canvas (hold **B** to flash the source in place — the Lightroom gesture);
/// `SideBySide` keeps the permanent comparison.
#[derive(Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ViewMode {
    SideBySide,
    AfterOnly,
}

/// A transient corner notification. Errors linger twice as long as successes —
/// a one-line status bar alone is too easy to miss for a failed export.
pub(crate) struct Toast {
    pub(crate) text: String,
    pub(crate) kind: ToastKind,
    pub(crate) born: Instant,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ToastKind {
    Success,
    Error,
}

impl Toast {
    /// Display time scales with READING time (阶段5 手感): a per-kind base
    /// plus 35 ms per char beyond the first 40, capped — a two-line path
    /// list used to vanish before anyone finished it, while "Saved" never
    /// needed its full window. Chars, not bytes: a hanzi is one read unit.
    pub(crate) fn ttl(&self) -> Duration {
        let extra = self.text.chars().count().saturating_sub(40) as u64 * 35;
        let (base, cap) = match self.kind {
            ToastKind::Success => (4_000, 10_000),
            ToastKind::Error => (8_000, 14_000),
        };
        Duration::from_millis((base + extra).min(cap))
    }
}

/// The prefs worth remembering across launches (stored via eframe persistence
/// next to the window geometry). Everything here must stay cheap to re-apply.
/// `serde(default)` so prefs saved by an older build (missing newer keys) still
/// load instead of silently resetting everything.
/// The delivery format+depth the Export panel dials in (round-12 阶段4:
/// the old `save_jpeg` bool could not say PNG, nor 8-bit TIFF). Stored in
/// Prefs as a small integer (`pref_code`); `save_jpeg` is still written
/// alongside for downgrade compatibility and consulted on load only when
/// the new code is absent (a pre-阶段4 prefs file).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ExportFormat {
    Tiff16,
    Tiff8,
    Png16,
    Png8,
    Jpeg,
}

impl ExportFormat {
    pub(crate) const ALL: [ExportFormat; 5] = [
        ExportFormat::Tiff16,
        ExportFormat::Tiff8,
        ExportFormat::Png16,
        ExportFormat::Png8,
        ExportFormat::Jpeg,
    ];

    /// English label — the i18n skeleton key (zh via `tr`; the audit
    /// extracts this fn's literals).
    pub(crate) fn label(self) -> &'static str {
        match self {
            ExportFormat::Tiff16 => "16-bit TIFF",
            ExportFormat::Tiff8 => "8-bit TIFF",
            ExportFormat::Png16 => "16-bit PNG",
            ExportFormat::Png8 => "8-bit PNG",
            ExportFormat::Jpeg => "JPEG",
        }
    }

    pub(crate) fn ext(self) -> &'static str {
        match self {
            ExportFormat::Tiff16 | ExportFormat::Tiff8 => "tif",
            ExportFormat::Png16 | ExportFormat::Png8 => "png",
            ExportFormat::Jpeg => "jpg",
        }
    }

    pub(crate) fn eight_bit(self) -> bool {
        matches!(self, ExportFormat::Tiff8 | ExportFormat::Png8 | ExportFormat::Jpeg)
    }

    pub(crate) fn pref_code(self) -> u8 {
        match self {
            ExportFormat::Tiff16 => 0,
            ExportFormat::Tiff8 => 1,
            ExportFormat::Png16 => 2,
            ExportFormat::Png8 => 3,
            ExportFormat::Jpeg => 4,
        }
    }

    /// Prefs migration: an unknown/absent code (0 is also serde's default)
    /// defers to the legacy `save_jpeg` bool, so a pre-阶段4 prefs file
    /// keeps the format the user had chosen.
    pub(crate) fn from_pref(code: u8, legacy_jpeg: bool) -> Self {
        match code {
            1 => ExportFormat::Tiff8,
            2 => ExportFormat::Png16,
            3 => ExportFormat::Png8,
            4 => ExportFormat::Jpeg,
            _ if legacy_jpeg => ExportFormat::Jpeg,
            _ => ExportFormat::Tiff16,
        }
    }
}

/// WHERE an export lands — the delivery target, promoted to a first-class
/// setting (R22-7). Before this it was not a setting at all: "Export" hardcoded
/// a cwd-relative `./out` and "Download…" always opened a save dialog, and the
/// TARGET was the only difference between those two toolbar buttons. One
/// setting + one split button replaces the pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum ExportDest {
    /// The DELIVERY ROOT — what the CLI, the web surface and the batch
    /// renderer all use, and still the default. `./out` beside the working
    /// directory until Settings moves it (R24-5 M8,
    /// [`autoshade::config::delivery_root`]); the label therefore names the
    /// ROLE, and the resolved absolute path is echoed beside it.
    #[default]
    OutFolder,
    /// Wherever the last export landed (`last_export_dir`). With nothing
    /// remembered yet this ASKS once and the answer seeds the memory.
    LastUsed,
    /// Ask every time (the old Download… behaviour, now a setting).
    Ask,
}

impl ExportDest {
    pub(crate) const ALL: [ExportDest; 3] =
        [ExportDest::OutFolder, ExportDest::LastUsed, ExportDest::Ask];

    /// English label — the i18n skeleton key (zh via `tr`; the audit extracts
    /// this fn's literals).
    pub(crate) fn label(self) -> &'static str {
        match self {
            ExportDest::OutFolder => "Delivery folder",
            ExportDest::LastUsed => "Last used folder",
            ExportDest::Ask => "Ask every time",
        }
    }

    pub(crate) fn pref_code(self) -> u8 {
        match self {
            ExportDest::OutFolder => 0,
            ExportDest::LastUsed => 1,
            ExportDest::Ask => 2,
        }
    }

    /// Prefs migration: 0 is also serde's default, so a prefs file written
    /// before R22-7 restores the historical `./out` behaviour, and an unknown
    /// code from a NEWER build degrades to it too rather than to "ask".
    pub(crate) fn from_pref(code: u8) -> Self {
        match code {
            1 => ExportDest::LastUsed,
            2 => ExportDest::Ask,
            _ => ExportDest::OutFolder,
        }
    }
}

/// What the Export button does THIS click, decided before anything is written
/// (R22-7). A separate type rather than an `Option<PathBuf>` so the asking
/// arm is a state a headless test can assert on — the dialog itself cannot be
/// opened from a test, but the decision that leads to it can.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum ExportRoute {
    /// The destination setting resolved to this concrete file.
    Render(PathBuf),
    /// The setting has to ask (Ask, or LastUsed with nothing remembered).
    Ask,
}

/// Persisted preference laws. Add a row and a restore arm for each new era.
///
/// | Era | Migration |
/// | --- | --- |
/// | 1 | The two denoise dials changed law in v1.4.0 (SCUNet split, default 0.5 → RAW-domain blend, default 1.0); a file written by v1.3.4–v1.5.0 carries its old numbers; step 0→1 resets both to the then-default 1.0. |
/// | 2 | Honest sigma and post-demosaic luminance return replace the old operating point; step 1→2 resets the RAW dials to the luminance-grain default and copies the old shared choices into separate baked-source slots. |
pub(crate) const PREFS_ERA: u32 = 2;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct Prefs {
    /// Missing on disk means era 0, even though a fresh default is current.
    #[serde(default)]
    pub(crate) prefs_era: u32,
    pub(crate) gallery_dir: Option<PathBuf>,
    /// Library OR direction for AI Analyze (2026-10-14). An older prefs file
    /// decodes to the default, `Library` — what an analysis without a typed
    /// direction already was.
    pub(crate) guide: autoshade::pipeline::Guide,
    pub(crate) style_strength: f32,
    /// R23-3: how COMMITTED the AI's grade should be, 0..1 — the second taste
    /// axis. `#[serde(default)]` on the struct would decode an older prefs file
    /// as 0.0, i.e. the MOST timid setting on a dial that exists because the AI
    /// was too timid, so [`Prefs::default`] supplies 0.65 and serde's
    /// struct-level default routes every missing key through it.
    pub(crate) grade_strength: f32,
    /// The reverse-fit's own Strength (user decision 2026-09-12). Routed through
    /// [`Prefs::default`] like `grade_strength`: a prefs file written before this
    /// key existed decodes to the lib's 0.65 — the byte-identical budget — and
    /// never to serde's 0.0.
    pub(crate) fit_strength: f32,
    /// R23-2: the opt-in style REFERENCE photo. `#[serde(default)]` on the
    /// struct means an older prefs file decodes it as `false` — the same
    /// answer as [`Prefs::default`], so an upgrade never silently starts
    /// spending an extra image per analysis.
    pub(crate) send_style_ref_image: bool,
    /// R23-4: deep thinking. `#[serde(default)]` on the struct decodes an older
    /// prefs file as `false` — the same answer [`Prefs::default`] gives, so an
    /// upgrade never silently starts paying for a deeper (and longer) analyze.
    pub(crate) deep_think: bool,
    /// Use SigLIP look embeddings when building and querying the style index.
    #[serde(default)]
    pub(crate) style_embed: bool,
    /// Step 14 / S2: also write a short LOCAL prose description of each
    /// record's grade with Qwen3-VL. `#[serde(default)]` on the struct decodes
    /// an older prefs file as `false` — the same answer [`Prefs::default`]
    /// gives, so an upgrade never silently starts a 4.3 GB download.
    #[serde(default)]
    pub(crate) style_describe: bool,
    /// The folder the reference library was last built from (R23-2), so a
    /// rebuild starts where the last build did.
    pub(crate) style_src_dir: Option<PathBuf>,
    /// The RAW folder for the sidecar settings check (2026-10-14).
    pub(crate) style_raw_dir: Option<PathBuf>,
    /// Direction adherence axis, persisted independently of style strength.
    #[serde(default = "default_adherence")]
    pub(crate) direction_adherence: f32,
    pub(crate) save_jpeg: bool,
    /// [`ExportFormat::pref_code`]; 0 defers to `save_jpeg` (migration).
    pub(crate) exp_format: u8,
    /// [`ExportDest::pref_code`]; 0 (serde's default too) = `./out`.
    pub(crate) exp_dest: u8,
    /// The folder the last export landed in — the target of
    /// [`ExportDest::LastUsed`], and where the ask-dialog reopens.
    pub(crate) last_export_dir: Option<PathBuf>,
    pub(crate) save_denoise: bool,
    /// The Export fold's own denoise strength and the Detail fold's own
    /// (2026-09-13) — two keys for two dials. Routed through
    /// [`Prefs::default`] like `fit_strength`: a prefs file written before
    /// these keys existed decodes to `denoise::DEFAULT_STRENGTH_RAW`, never to
    /// serde's 0.0 (which is the identity — every denoise would silently do
    /// nothing after an upgrade).
    pub(crate) save_denoise_strength: f32,
    pub(crate) denoise_strength: f32,
    /// SCUNet choices are independent of RAW dial-law migrations.
    pub(crate) baked_denoise_strength: f32,
    pub(crate) baked_save_denoise_strength: f32,
    pub(crate) zoned_fit: bool,
    /// Opt-in expansion from the historical two-region sky/land pass.
    pub(crate) zoned_four_regions: bool,
    pub(crate) fit_ai_judge: bool,
    /// R23-6: the DEEP reverse-fit (judge before the persist, bounded guided
    /// rounds). `#[serde(default)]` on the struct decodes an older prefs file
    /// as `false` — the same answer [`Prefs::default`] gives, so an upgrade
    /// never silently starts spending vision calls on every reverse-fit.
    pub(crate) fit_deep: bool,
    /// Step-6: the generation-side fidelity retry — when a reimagine's
    /// structural divergence D reaches the reverse-fit threshold, buy ONE
    /// more generation and keep the closer result. `#[serde(default)]` on
    /// the struct decodes an older prefs file as `false` — the same answer
    /// [`Prefs::default`] gives, so an upgrade never silently starts buying
    /// a second image per reimagine.
    pub(crate) reimagine_retry: bool,
    /// v1.17.0: ask whether to reverse-fit after a whole-frame「AI generate」
    /// lands. Routed through [`Prefs::default`] like `fit_strength`: a prefs
    /// file written before this key existed decodes to `true`, the question
    /// every user is shown first, never to serde's `false`.
    pub(crate) ask_fit_after_generate: bool,
    /// The adjust fold's own quality; old prefs default to high (0).
    pub(crate) adjust_quality: usize,
    pub(crate) view_mode: ViewMode,
    pub(crate) exp_long_edge: u32,
    pub(crate) exp_sharpen: f32,
    pub(crate) exp_quality: f32,
    pub(crate) exp_space: u8,
    pub(crate) preview_edge: u32,
    pub(crate) show_clipping: bool,
    pub(crate) lang: Lang,
    pub(crate) theme: ThemePref,
    pub(crate) accent: AccentPref,
}

impl Default for Prefs {
    fn default() -> Self {
        // Mirror AutoShadeApp's own defaults (see its Default impl) so a pref
        // key missing from an older save degrades to exactly the app default.
        Self {
            prefs_era: PREFS_ERA,
            gallery_dir: None,
            guide: autoshade::pipeline::Guide::default(),
            style_strength: STYLE_STRENGTH_DEFAULT,
            grade_strength: GRADE_STRENGTH_DEFAULT,
            fit_strength: autoshade::recipe::GradeStrength::DEFAULT,
            // OFF, like every other paid opt-in here (see `fit_ai_judge`).
            send_style_ref_image: false,
            deep_think: false,
            style_embed: false,
            style_describe: false,
            style_src_dir: None,
            style_raw_dir: None,
            direction_adherence: autoshade::recipe::DirectionAdherence::DEFAULT,
            save_jpeg: false,
            exp_format: 0,
            exp_dest: 0, // ./out — the CLI/batch shape, unchanged for old prefs
            last_export_dir: None,
            save_denoise: false,
            save_denoise_strength: autoshade::denoise::DEFAULT_STRENGTH_RAW,
            denoise_strength: autoshade::denoise::DEFAULT_STRENGTH_RAW,
            baked_denoise_strength: autoshade::denoise::DEFAULT_STRENGTH,
            baked_save_denoise_strength: autoshade::denoise::DEFAULT_STRENGTH,
            // Zoned sky reverse-fit ON by default: it degrades gracefully to
            // the plain global fit when segmentation is unavailable.
            zoned_fit: true,
            zoned_four_regions: false,
            // AI review of the fit OFF by default: it is a PAID vision call
            // per fit — spending is the user's opt-in, never a default.
            fit_ai_judge: false,
            // The deep reverse-fit is the same rule and a bigger bill (up to
            // three vision calls per fit), so the same answer.
            fit_deep: false,
            // A second billed generation per diverged reimagine — same rule.
            reimagine_retry: false,
            // Free to answer, and 「Don't ask again」 is one click inside it.
            ask_fit_after_generate: true,
            adjust_quality: 0,
            view_mode: ViewMode::SideBySide,
            exp_long_edge: 0,
            exp_sharpen: 0.0,
            exp_quality: 95.0,
            exp_space: 0,
            preview_edge: PREVIEW_EDGE,
            show_clipping: false,
            lang: Lang::En, // English is the default / skeleton language
            theme: ThemePref::Dark, // the historical look
            accent: AccentPref::Gold,
        }
    }
}

/// Where the personal-style strength starts, what its slider's double-click /
/// right-click reset lands on, and the baseline the AI section's ● compares
/// against ([`AutoShadeApp::ai_section_active`]). ONE definition on purpose
/// (R22 #16): the number was a bare `0.30` in BOTH `Default` impls, so
/// "the user moved it" was a statement no predicate could make without adding a
/// third copy to drift from the other two.
pub(crate) const STYLE_STRENGTH_DEFAULT: f32 = 0.30;

fn default_adherence() -> f32 { autoshade::recipe::DirectionAdherence::DEFAULT }

/// Where the GRADE strength starts, and what its slider's double-click /
/// right-click reset lands on (R23-3; user decision 2026-08-17 ⑦ = 0.65).
///
/// An ALIAS of the library's own constant, never a second literal: the lib uses
/// it for the CLI's omitted `--strength`, the web body's absent field and
/// `GradeStrength::default()`, so a GUI copy would let the desktop app develop a
/// photo differently from `autoshade analyze` on the same untouched settings.
/// `GradeStrength::CALIBRATED` (0.50) is the other named point on this dial —
/// the calibrated guardrail NUMBERS, one drag away. Not the whole pre-R23
/// request: that release's restraint wording is the ≤ 0.40 band's prose (see
/// `GradeStrength::CALIBRATED`).
pub(crate) const GRADE_STRENGTH_DEFAULT: f32 = autoshade::recipe::GradeStrength::DEFAULT;

pub(crate) const PREVIEW_EDGE: u32 = 1280; // working preview size for fast live develop

pub(crate) const THUMB_EDGE: u32 = 160; // decoded gallery-thumbnail long edge

// The GALLERY's thumbnail geometry: decode budget (THUMB_EDGE) → drawn slot
// (THUMB_W × THUMB_H) → row height, one pipeline, which is why these four sit
// together rather than in theme.rs beside the cross-surface spacing/width
// SCALES. The variant strip's own thumb height is a different surface with its
// own arithmetic and lives at its one consumer (`STRIP_THUMB_H` in
// panels/develop.rs::variant_strip, which names this constant back).
pub(crate) const THUMB_W: f32 = 56.0; // displayed thumbnail size in the gallery

pub(crate) const THUMB_H: f32 = 40.0;

pub(crate) const GALLERY_ROW_H: f32 = 50.0; // fixed row height for ScrollArea::show_rows

pub(crate) const MAX_THUMB_INFLIGHT: usize = 6; // cap concurrent thumbnail decodes

// ONE band table (L16-7): the GUI indexes the recipe's HSL arrays with
// this order, so it must BE the recipe's order — a literal copy could be
// reordered on either side and silently mislabel every band.
pub(crate) use autoshade::recipe::HSL_BANDS;

// Representative swatch per HSL band (row markers in the mixer — approximate
// band-centre hues, display-only; the engine's band maths lives in render.rs).
pub(crate) const HSL_SWATCH: [egui::Color32; 8] = [
    egui::Color32::from_rgb(224, 72, 60),  // Red
    egui::Color32::from_rgb(224, 141, 60), // Orange
    egui::Color32::from_rgb(224, 211, 60), // Yellow
    egui::Color32::from_rgb(76, 191, 90),  // Green
    egui::Color32::from_rgb(60, 200, 200), // Aqua
    egui::Color32::from_rgb(76, 120, 224), // Blue
    egui::Color32::from_rgb(142, 90, 217), // Purple
    egui::Color32::from_rgb(208, 82, 184), // Magenta
];


/// The tool a painted area belongs to. Each tool keeps its own strokes
/// (user decision 2026-09-27, 「各区各一块」): the area painted for Heal is
/// not what Fill, the Stamp or Adjust read, and each fold carries its own
/// 「🖌 Paint area」 / 「Clear area」 pair. The canvas (`mask_paint`) holds the
/// selected owner's area; the others wait in `paint_store`. The Local-Masks
/// brush is a SESSION on the canvas, not an owner: its strokes bake into a
/// raster, and the owner's area is stashed for the session's duration
/// (`stash_owner_area` / `unstash_owner_area`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum BrushOwner {
    #[default]
    Fill,
    Heal,
    Stamp,
    Adjust,
}

impl BrushOwner {
    pub(crate) const COUNT: usize = 4;

    /// The store slot of this owner's area while it is off the canvas.
    pub(crate) fn slot(self) -> usize {
        match self {
            BrushOwner::Fill => 0,
            BrushOwner::Heal => 1,
            BrushOwner::Stamp => 2,
            BrushOwner::Adjust => 3,
        }
    }
}

/// One tool's painted area while it is off the canvas: the red display
/// canvas and the same presence memo `has_painted_mask` keeps for the live
/// one (None = one scan is due; a swap back restores it unscanned).
pub(crate) struct PaintArea {
    pub(crate) canvas: image::RgbaImage,
    pub(crate) painted: std::cell::Cell<Option<bool>>,
}

/// How a finished retouch enters the variant strip.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum RetouchKind {
    /// A reimagine or adjust → a NEW「AI 生成」variant (its look lives in
    /// the pixels).
    NewGenerated,
    /// A generative fill → a LAYER on the card it was made on (2026-10-01,
    /// user decision; it was a new ✨ card from v1.3.5): the saved RGBA patch
    /// joins that card's `recipe.pixel_layers`, over its develop, and the
    /// card's own pixels never change (`render::PixelLayer`).
    Layer,
    /// An AI denoise → a NEW「◈ Denoised negative」card (2026-09-15, user
    /// decision): the saved master is its `origin`, the recipe of the card it
    /// was made from is its recipe, and the card it was made from — the ▣
    /// negative included — keeps its own pixels. An in-place denoise used to
    /// redefine the ▣ card and left the original one Ctrl+Z away, per session.
    NewDenoised,
    /// A stack/merge → a NEW「▦ Stacked」card (v1.5.0 Track S), on exactly the
    /// ◈ card's terms: the merged master is its `origin`, this card's develop
    /// is its recipe, and the frame it was stacked FROM keeps its own pixels.
    /// The two land through one arm, because they are one idea — a new
    /// negative made from the old one, standing beside it.
    NewStacked,
    /// A heal/clone touch-up of the CURRENT rendition → bake into the
    /// active variant's base AND repoint its `origin` at the saved artifact, so
    /// export / reverse-fit / a further retouch all follow the retouched pixels
    /// (WYSIWYG) instead of the pre-retouch source.
    InPlace,
}

pub(crate) use crate::logic::retouch::RetouchNote;

/// The generative tools and the recipe clipboard: the fill/heal dials and
/// prompts, the reimagine/adjust prompts and quality, the in-app recipe
/// clipboard (what and from where), the paste-geometry switch and the working-
/// preview long edge. Two of these are also `Prefs` fields; save/load maps them.
pub(crate) struct GenerativeState {
    pub(crate) fill_prompt: String,
    pub(crate) fill_quality: usize,
    pub(crate) fill_fullres: bool,
    pub(crate) heal_fullres: bool,
    pub(crate) reimagine_prompt: String,
    pub(crate) adjust_prompt: String,
    pub(crate) adjust_edit_prompt: String,
    pub(crate) adjust_quality: usize,
    pub(crate) copied: Option<EditRecipe>,
    pub(crate) copied_from: Option<PathBuf>,
    pub(crate) paste_geometry: bool,
    pub(crate) preview_edge: u32,
}

impl Default for GenerativeState {
    fn default() -> Self {
        Self {
            fill_prompt: String::new(),
            fill_quality: 0,
            fill_fullres: false,
            heal_fullres: false,
            reimagine_prompt: String::new(),
            adjust_prompt: String::new(),
            adjust_edit_prompt: String::new(),
            adjust_quality: 0,
            copied: None,
            copied_from: None,
            paste_geometry: false,
            preview_edge: PREVIEW_EDGE,
        }
    }
}

/// The canvas tools and the AI/generation flight flags: the four eyedroppers,
/// the clone brush, the stack merge dials, the generation epoch/cancel token,
/// whether an analyze is in flight, and the Look protocol's state.
pub(crate) struct ToolsState {
    pub(crate) wb_picking: bool,
    pub(crate) range_picking: Option<usize>,
    pub(crate) point_color_picking: bool,
    pub(crate) lens_focus_picking: bool,
    pub(crate) clone_mode: bool,
    pub(crate) clone_src: Option<(f32, f32)>,
    pub(crate) clone_fullres: bool,
    pub(crate) stack_kind: autoshade::stack::merge::StackKind,
    pub(crate) stack_tripod: bool,
    pub(crate) gen_epoch: u64,
    pub(crate) gen_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub(crate) analyze_inflight: bool,
    pub(crate) look: crate::logic::look::LookState,
}

impl Default for ToolsState {
    fn default() -> Self {
        Self {
            wb_picking: false,
            range_picking: None,
            point_color_picking: false,
            lens_focus_picking: false,
            clone_mode: false,
            clone_src: None,
            clone_fullres: false,
            stack_kind: autoshade::stack::merge::StackKind::Hdr,
            stack_tripod: false,
            gen_epoch: 0,
            gen_cancel: None,
            analyze_inflight: false,
            look: Default::default(),
        }
    }
}

/// The window shell: the Settings form, the theme/accent actually installed,
/// which windows are open, the panel/defocus switches, the quit confirmation,
/// the first-frame theme sync and the Python-environment probe. The UI language
/// stays on the app — every `tr()` call reads it.
pub(crate) struct ShellState {
    pub(crate) settings: SettingsForm,
    pub(crate) theme: ThemePref,
    pub(crate) accent: AccentPref,
    pub(crate) show_settings: bool,
    pub(crate) show_shortcuts: bool,
    pub(crate) show_analysis_full: bool,
    pub(crate) panels_hidden: bool,
    pub(crate) defocus_next: bool,
    pub(crate) confirm_quit: bool,
    pub(crate) os_theme_synced: bool,
    pub(crate) python_env_line: Option<String>,
    pub(crate) python_check: Option<autoshade::python_env::PythonCheck>,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            settings: SettingsForm::default(),
            theme: ThemePref::Dark,
            accent: AccentPref::Gold,
            show_settings: false,
            show_shortcuts: false,
            show_analysis_full: false,
            panels_hidden: false,
            defocus_next: false,
            confirm_quit: false,
            os_theme_synced: false,
            python_env_line: None,
            python_check: None,
        }
    }
}

/// The status chrome around the workspace: the startup greeting, the toast
/// queue, the histogram cache, the window-title cache and the clipping
/// warnings (the shown flag, the last pixels and their texture). The status
/// line itself stays on the app — it is a per-frame read for nearly every
/// action.
#[derive(Default)]
pub(crate) struct ChromeState {
    pub(crate) startup_note: Option<String>,
    pub(crate) toasts: Vec<Toast>,
    pub(crate) histogram: Option<Vec<[f32; 4]>>,
    pub(crate) last_title: String,
    pub(crate) show_clipping: bool,
    pub(crate) last_rgb: Option<image::RgbImage>,
    pub(crate) clip_tex: Option<egui::TextureHandle>,
}

impl ChromeState {
    /// Push a toast, refreshing an identical LIVE one instead of duplicating:
    /// undoing through a washed history region fires the same repair
    /// disclosure once per healed step, and five copies evicted still-live
    /// ERROR toasts from the ring. The refresh MOVES it to the back — it is
    /// the newest content, and refreshing in place left it first in line for
    /// eviction, the exact loss the dedup exists to prevent.
    pub(crate) fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        let text = text.into();
        if let Some(i) = self.toasts.iter().position(|t| t.text == text && t.kind == kind) {
            self.toasts.remove(i);
        }
        self.toasts.push(Toast { text, kind, born: std::time::Instant::now() });
        if self.toasts.len() > 5 {
            self.toasts.remove(0); // keep the stack readable
        }
    }
}


/// The named-recipe library panel: whether it is open, the loaded list and its
/// read/filter/selection, the name field, the inline rename and delete-armed
/// rows, and the preview pane's texture. Extracted from `AutoShadeApp` so
/// recipe-library work does not add fields to the app struct.
#[derive(Default)]
pub(crate) struct RecipeLibraryState {
    pub(crate) show_recipes: bool,
    pub(crate) recipes: Vec<autoshade::recipe_library::Stored>,
    pub(crate) recipes_loaded: bool,
    pub(crate) recipes_filter: String,
    pub(crate) recipe_sel: Option<String>,
    pub(crate) recipe_name_buf: String,
    pub(crate) recipe_rename: Option<(String, String)>,
    pub(crate) recipe_delete_armed: Option<String>,
    pub(crate) recipe_preview: Option<RecipePreview>,
}


/// The reverse-fit state: the persisted dials (zoned fit, the four-region
/// expansion, the AI judge, deep think, reimagine retry, ask-after-generate)
/// plus the reference path, its label and the prompt dialog. Five of the dials
/// are also `Prefs` fields; the save/load code maps between them.
pub(crate) struct FitState {
    pub(crate) zoned_fit: bool,
    pub(crate) zoned_four_regions: bool,
    pub(crate) fit_ai_judge: bool,
    pub(crate) fit_deep: bool,
    pub(crate) reimagine_retry: bool,
    pub(crate) ask_fit_after_generate: bool,
    pub(crate) fit_ref: Option<PathBuf>,
    pub(crate) fit_from: Option<String>,
    pub(crate) fit_prompt: Option<FitPrompt>,
}

impl Default for FitState {
    fn default() -> Self {
        Self {
            zoned_fit: true,
            zoned_four_regions: false,
            fit_ai_judge: false,
            fit_deep: false,
            reimagine_retry: false,
            ask_fit_after_generate: true,
            fit_ref: None,
            fit_from: None,
            fit_prompt: None,
        }
    }
}

/// The card strip's dialog/one-shot state: the snapshot numbers and their meta,
/// the version name dialog and its filter, the variant name/delete dialogs and
/// the one-shot keep-recipe flag a preview re-decode sets. The variant list
/// itself stays on the app (it is the open photo's core state).
#[derive(Default)]
pub(crate) struct CardsState {
    pub(crate) keep_recipe: bool,
    pub(crate) versions: Vec<u32>,
    pub(crate) version_meta: HashMap<u32, autoshade::store::VersionMetaEntry>,
    pub(crate) version_name_buf: Option<(PathBuf, u32, String, String)>,
    pub(crate) versions_current_only: bool,
    pub(crate) variant_name_buf: Option<(PathBuf, String, String, String)>,
    pub(crate) variant_delete_confirm: Option<usize>,
}


/// The persisted AI preferences, mirrored on the app so the panels read them
/// without loading `Prefs`: the guide, the three strengths, the adherence dial
/// and the library/deep-think switches. Field names are the app's; the `Prefs`
/// struct keeps its own serialized names, and the load/save code maps between
/// them explicitly.
pub(crate) struct AiPrefs {
    pub(crate) guide: autoshade::pipeline::Guide,
    pub(crate) style_strength: f32,
    pub(crate) grade_strength: f32,
    pub(crate) fit_strength: f32,
    pub(crate) send_style_ref_image: bool,
    pub(crate) deep_think: bool,
    pub(crate) style_embed: bool,
    pub(crate) style_describe: bool,
    pub(crate) style_src_dir: Option<PathBuf>,
    pub(crate) style_raw_dir: Option<PathBuf>,
    pub(crate) direction_adherence: f32,
}

impl Default for AiPrefs {
    fn default() -> Self {
        Self {
            guide: autoshade::pipeline::Guide::default(),
            style_strength: STYLE_STRENGTH_DEFAULT,
            grade_strength: GRADE_STRENGTH_DEFAULT,
            fit_strength: autoshade::recipe::GradeStrength::DEFAULT,
            send_style_ref_image: false,
            deep_think: false,
            style_embed: false,
            style_describe: false,
            style_src_dir: None,
            style_raw_dir: None,
            direction_adherence: autoshade::recipe::DirectionAdherence::DEFAULT,
        }
    }
}

/// The open/photo-source state: the navigation stash, the master cache and its
/// loads, the source part and its re-develop flight, the photo's lens/as-shot
/// facts, the Before baseline, the open window's flags and the discard request.
/// Extracted from `AutoShadeApp` so open-path work does not add fields to the
/// app struct.
#[derive(Default)]
pub(crate) struct OpenState {
    pub(crate) nav_stash: HashMap<PathBuf, StashEntry>,
    pub(crate) master_loads: std::collections::HashSet<(PathBuf, PathBuf)>,
    pub(crate) master_cache: Vec<((PathBuf, u32, FileStamp), Arc<image::DynamicImage>)>,
    pub(crate) open_unresolved: bool,
    pub(crate) pasted_open: Option<(PathBuf, EditRecipe)>,
    pub(crate) photo_start: autoshade::pipeline::CanvasStart,
    pub(crate) source_part: autoshade::render::BasePart,
    pub(crate) source_part_inflight: Option<autoshade::render::BasePart>,
    pub(crate) photo_lens: autoshade::recipe::LensProfile,
    pub(crate) photo_as_shot: Option<(f32, f32)>,
    pub(crate) before_start: EditRecipe,
    pub(crate) base_cache: Vec<(BaseCacheKey, OpenedBase)>,
    pub(crate) open_in_flight: bool,
    pub(crate) open_same_path: bool,
    pub(crate) edge_before_flight: Option<u32>,
    pub(crate) discard_requested: bool,
}


/// The AI section's non-persisted state: the reference library's summary and
/// build flight, the free-text direction, and the Color Mixer / grade tab
/// indices the panel remembers. Extracted from `AutoShadeApp` so AI-panel work
/// does not add fields to the app struct.
#[derive(Default)]
pub(crate) struct AiState {
    pub(crate) style_info: Option<autoshade::library::Summary>,
    pub(crate) style_info_loading: bool,
    pub(crate) style_build_inflight: bool,
    pub(crate) style_build_progress: Option<(autoshade::library::BuildStage, usize, usize)>,
    pub(crate) guidance: String,
    pub(crate) hsl_tab: usize,
    pub(crate) grade_region: usize,
}


/// Layout/behaviour seams the window tests read: measured rects and the
/// actual enabled state of gated buttons. Compiled only under `cfg(test)`, so
/// a test's new probe never adds a field to `AutoShadeApp`'s production shape.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestSeams {
    pub(crate) curve_rect: Option<egui::Rect>,
    pub(crate) strip_row_rect: Option<egui::Rect>,
    pub(crate) strip_card_rect: Option<egui::Rect>,
    pub(crate) strip_title_rect: Option<egui::Rect>,
    pub(crate) strip_apply: Option<(egui::Rect, bool)>,
    pub(crate) strip_name_rect: Option<egui::Rect>,
    pub(crate) edit_list_rows: Vec<String>,
    pub(crate) edit_list_actions: Vec<String>,
    pub(crate) reimagine_btn_rect: Option<egui::Rect>,
    pub(crate) fit_from_rect: Option<egui::Rect>,
    pub(crate) adjust_btn_enabled: Option<bool>,
    pub(crate) brush_slider_rect: Option<egui::Rect>,
    pub(crate) analysis_box_rect: Option<egui::Rect>,
    /// The last denoise strength a verb captured: (on_export, strength).
    pub(crate) denoise_strength_asked: Option<(bool, f32)>,
    pub(crate) histogram_rect: Option<egui::Rect>,
    pub(crate) prompt_rects: Vec<egui::Rect>,
    pub(crate) ai_gate_enabled: Option<bool>,
    pub(crate) adjust_edit_btn_enabled: Option<bool>,
    pub(crate) adherence_gate_enabled: Option<bool>,
    pub(crate) guide_gate_enabled: Option<(bool, bool)>,
    pub(crate) ai_library_read_enabled: Option<bool>,
    pub(crate) ai_library_build_enabled: Option<bool>,
    pub(crate) ai_mask_stub: Option<crate::persist::AiMaskRunner>,
}

/// The crop tool's state: whether the overlay is armed, the preset index and
/// its pending re-derive, the guide style and the drag in flight. Extracted
/// from `AutoShadeApp` so crop work does not add fields to the app struct.
#[derive(Default)]
pub(crate) struct CropState {
    pub(crate) mode: bool,
    pub(crate) aspect: usize,
    pub(crate) aspect_pending: bool,
    pub(crate) grid: u8,
    pub(crate) drag: Option<(u8, egui::Pos2, [f32; 4], f32)>,
}


/// The canvas's view state: zoom and pan (their glide target and the 1:1 scale),
/// the controls-panel rect the arrow keys read, the latched Before view and the
/// compare/single switch. Extracted from `AutoShadeApp` so view features do not
/// add fields to the app struct.
pub(crate) struct ViewState {
    pub(crate) zoom: f32,
    pub(crate) pan: egui::Vec2,
    pub(crate) one_to_one: f32,
    pub(crate) zoom_target: f32,
    pub(crate) controls_rect: Option<egui::Rect>,
    pub(crate) before: bool,
    pub(crate) mode: ViewMode,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: egui::vec2(0.5, 0.5),
            one_to_one: 1.0,
            zoom_target: 1.0,
            controls_rect: None,
            before: false,
            mode: ViewMode::SideBySide,
        }
    }
}

/// The painting tools and the mask list's edit state: the brush dials, the
/// paint buffer and its per-owner stash, the mask-brush session, and the
/// selected mask/component plus the placement gesture. Extracted from
/// `AutoShadeApp` so mask/paint work does not add fields to the app struct.
pub(crate) struct MaskEditState {
    pub(crate) mask_name_buf: Option<(usize, String, String)>,
    pub(crate) paint_mode: bool,
    pub(crate) brush: f32,
    pub(crate) brush_resized_at: Option<f64>,
    pub(crate) brush_feather: f32,
    pub(crate) brush_flow: f32,
    pub(crate) mask_paint: Option<image::RgbaImage>,
    pub(crate) mask_painted: std::cell::Cell<Option<bool>>,
    pub(crate) paint_owner: BrushOwner,
    pub(crate) paint_store: [Option<PaintArea>; BrushOwner::COUNT],
    pub(crate) paint_last: Option<(f32, f32)>,
    pub(crate) mask_brush: Option<(Option<usize>, bool)>,
    pub(crate) mask_brush_gray: Option<image::GrayImage>,
    pub(crate) sel_mask: Option<usize>,
    pub(crate) placing_mask: Option<(MaskKind, PlaceTarget)>,
    pub(crate) sel_component: Option<usize>,
    pub(crate) component_mode: autoshade::recipe::MaskCombine,
    pub(crate) place_start: Option<(f32, f32)>,
    pub(crate) mask_drag: Option<(u8, (f32, f32))>,
}

impl Default for MaskEditState {
    fn default() -> Self {
        Self {
            mask_name_buf: None,
            paint_mode: false,
            brush: 30.0,
            brush_resized_at: None,
            brush_feather: 0.0,
            brush_flow: 1.0,
            mask_paint: None,
            mask_painted: std::cell::Cell::new(None),
            paint_owner: BrushOwner::Fill,
            paint_store: Default::default(),
            paint_last: None,
            mask_brush: None,
            mask_brush_gray: None,
            sel_mask: None,
            placing_mask: None,
            sel_component: None,
            component_mode: autoshade::recipe::MaskCombine::Add,
            place_start: None,
            mask_drag: None,
        }
    }
}

/// The mask-coverage overlay and the box-selected AI region: the texture, its
/// rebuild key/throttle, the dirty sub-rect, the shown switch, the hovered row
/// and the cached range reference, plus the region gesture. Extracted from
/// `AutoShadeApp` so canvas-overlay work does not add fields to the app struct.
pub(crate) struct OverlayState {
    pub(crate) region: Option<[f32; 4]>,
    pub(crate) region_drag: Option<(egui::Pos2, egui::Pos2)>,
    pub(crate) region_restore: Option<([f32; 4], Instant)>,
    #[cfg(test)]
    pub(crate) mask_presence_scans: std::cell::Cell<usize>,
    pub(crate) mask_tex: Option<egui::TextureHandle>,
    pub(crate) mask_dirty: bool,
    pub(crate) mask_tex_xform: (f32, f32, (bool, bool), (f32, f32)),
    pub(crate) mask_dirty_rect: Option<[u32; 4]>,
    pub(crate) mask_tex_built: Instant,
    pub(crate) show_mask_overlay: bool,
    pub(crate) mask_overlay_tex: Option<egui::TextureHandle>,
    pub(crate) overlay_stale: bool,
    pub(crate) overlay_key: Option<OverlayKey>,
    pub(crate) hover_mask: Option<usize>,
    pub(crate) overlay_ref: Option<(EditRecipe, image::DynamicImage)>,
    pub(crate) overlay_build_count: u64,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            region: None,
            region_drag: None,
            region_restore: None,
            #[cfg(test)]
            mask_presence_scans: std::cell::Cell::new(0),
            mask_tex: None,
            mask_dirty: false,
            mask_tex_xform: (0.0, 0.0, (false, false), (0.0, 0.0)),
            mask_dirty_rect: None,
            mask_tex_built: Instant::now(),
            show_mask_overlay: true,
            mask_overlay_tex: None,
            overlay_stale: false,
            overlay_key: None,
            hover_mask: None,
            overlay_ref: None,
            overlay_build_count: 0,
        }
    }
}

/// The undo history and the ●-baseline mirror: the two stacks, the committed
/// step, and the recipe/strip/pixel-link the store last holds for the open
/// card. Extracted from `AutoShadeApp` so history features do not add fields
/// to the app struct.
#[derive(Default)]
pub(crate) struct HistoryState {
    pub(crate) undo_stack: Vec<UndoStep>,
    pub(crate) redo_stack: Vec<UndoStep>,
    pub(crate) committed: UndoStep,
    pub(crate) saved_recipe: EditRecipe,
    pub(crate) saved_strip: Option<autoshade::store::VariantsRecord>,
    pub(crate) pixels_on_disk: Option<PathBuf>,
}

/// The Export panel's dials, the delivery target and the save-denoise options
/// they drive. Extracted from `AutoShadeApp` so export features do not add
/// fields to the app struct.
pub(crate) struct ExportState {
    pub(crate) format: ExportFormat,
    pub(crate) dest: ExportDest,
    pub(crate) last_dir: Option<PathBuf>,
    pub(crate) xmp_beside_confirm: bool,
    pub(crate) batch_progress: Option<(usize, usize)>,
    pub(crate) long_edge: u32,
    pub(crate) sharpen: f32,
    pub(crate) quality: f32,
    pub(crate) space: u8,
    pub(crate) save_denoise: bool,
    pub(crate) save_denoise_strength: f32,
    pub(crate) denoise_strength: f32,
    pub(crate) baked_denoise_strength: f32,
    pub(crate) baked_save_denoise_strength: f32,
}

impl Default for ExportState {
    fn default() -> Self {
        Self {
            format: ExportFormat::Tiff16,
            dest: ExportDest::OutFolder,
            last_dir: None,
            xmp_beside_confirm: false,
            batch_progress: None,
            long_edge: 0,
            sharpen: 0.0,
            quality: 95.0,
            space: 0,
            save_denoise: false,
            save_denoise_strength: autoshade::denoise::DEFAULT_STRENGTH_RAW,
            denoise_strength: autoshade::denoise::DEFAULT_STRENGTH_RAW,
            baked_denoise_strength: autoshade::denoise::DEFAULT_STRENGTH,
            baked_save_denoise_strength: autoshade::denoise::DEFAULT_STRENGTH,
        }
    }
}

/// The gallery's own state: the folder scan, the bounded thumbnail cache, the
/// selection, and the two measured rects the layout tests read. Extracted from
/// `AutoShadeApp` so gallery features do not add fields to the app struct.
#[derive(Default)]
pub(crate) struct GalleryState {
    pub(crate) gallery: Vec<PathBuf>, // sources in the working folder (sorted)
    pub(crate) gallery_dir: Option<PathBuf>, // the working folder
    pub(crate) gallery_gen: u64, // bumped on every folder load (thumb invalidation)
    pub(crate) gallery_scroll_to: Option<usize>,
    pub(crate) thumbs: HashMap<usize, egui::TextureHandle>, // decoded thumbnails by index
    pub(crate) thumb_requested: HashSet<usize>, // indices already queued/decoded
    pub(crate) thumb_fail: std::collections::HashMap<usize, u8>,
    pub(crate) thumb_inflight: usize, // live thumbnail-decode threads
    pub(crate) edited_badge: HashMap<usize, bool>, // cached "● edited" sidecar stat per index
    pub(crate) selected: Option<usize>, // index of the open gallery photo (for highlight)
    pub(crate) multi_sel: HashSet<usize>, // Ctrl+click gallery multi-selection
    #[cfg(test)]
    pub(crate) gallery_slot_rect: Option<egui::Rect>, // test seam: first drawn gallery thumb's SLOT
    #[cfg(test)]
    pub(crate) gallery_thumb_rect: Option<egui::Rect>, // test seam: … and the image drawn inside it
}

/// The question a whole-frame「AI generate」landing asks (v1.17.0): reverse-fit
/// the new ✨ card, and from which negative (`fit_prompt_layer`).
pub(crate) struct FitPrompt {
    /// The ✨ card the answer is about, by id — the strip switched to it, and
    /// a newer generation landing while the question is up takes it over.
    pub(crate) target: String,
    /// That generation's structural divergence D, for the fidelity line.
    pub(crate) divergence: f32,
    /// The source picked in the prompt's「From:」menu (`None` = Automatic);
    /// it starts on the ▣ original.
    pub(crate) from: Option<String>,
    /// The「Don't ask again」box, written to the pref when the prompt closes.
    pub(crate) dont_ask: bool,
}

impl RetouchNote {
    /// The painted area this result consumed, cleared at the landing so the
    /// tool starts clean for its next use; a whole-frame result (reimagine,
    /// denoise, stack) painted nothing and clears nothing.
    pub(crate) fn consumed_area(&self) -> Option<BrushOwner> {
        match self {
            RetouchNote::Filled(_) => Some(BrushOwner::Fill),
            RetouchNote::Adjusted { .. } => Some(BrushOwner::Adjust),
            RetouchNote::Healed { .. } => Some(BrushOwner::Heal),
            RetouchNote::Cloned { .. } => Some(BrushOwner::Stamp),
            RetouchNote::FoundFilled { .. }
            | RetouchNote::Stacked { .. }
            | RetouchNote::Denoised { .. }
            | RetouchNote::Reimagined { .. } => None,
        }
    }
}

/// A finished retouch from the pixel paths (fill/adjust/heal/denoise/stack/
/// clone/reimagine): `(preview of the ./out result, typed note, the saved
/// full-resolution pixel master, kind)`. The saved path becomes the affected
/// variant's `origin` — its export / reverse-fit / next-retouch source.
pub(crate) type RetouchDone =
    anyhow::Result<(image::DynamicImage, RetouchNote, PathBuf, RetouchKind)>;

/// Style-prompt extraction facts (L12#4): whether the ./out copy landed.
pub(crate) enum StyleNote {
    SavedCopy,
    SaveFailed(String),
    NotSaved,
}

pub(crate) use crate::logic::export_landing::ExportOutcome;

/// How a single delivery treats a file already at its path
/// (`start_render_with`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutName {
    /// The library guard decides (the workspace's Export).
    Guarded,
    /// A name the app made itself (Normal's output-folder save): never
    /// replace a file there — claim the first free `-2`, `-3`, … beside it.
    Numbered,
    /// A file the user picked in the save window, which confirms a replace
    /// itself (Normal): theirs to replace, so it is not numbered.
    Chosen,
}

/// Batch recipe-paste facts (L12#4). `errs` non-empty = partial failure —
/// the landing must keep routing that through the error channel.
pub(crate) struct PasteOutcome {
    pub(crate) ok: usize,
    pub(crate) xmp: usize,
    pub(crate) errs: Vec<String>,
    pub(crate) xmp_fails: Vec<String>,
    pub(crate) xmp_notes: Vec<String>,
    /// The photos whose recipe the paste wrote — the landing drops their
    /// navigation stash, which the written look replaced.
    pub(crate) written: Vec<PathBuf>,
}

/// CPU-built preview frame. Everything expensive is worker-side: engine
/// develop, geometry, the one RGB8 conversion, histogram, clipping pixels and
/// the 96px variant thumbnail. The UI thread only submits the prepared images
/// to egui textures. `base` + `recipe` are identity tags: if either differs
/// when the result arrives, the frame is stale and is discarded (latest wins).
pub(crate) struct PreviewDone {
    pub(crate) base: Arc<image::DynamicImage>,
    pub(crate) recipe: EditRecipe,
    /// Kept AFTER display too (as `last_rgb`) so toggling the clipping layer
    /// can rebuild its overlay without a full redevelop.
    pub(crate) rgb: image::RgbImage,
    /// The texture-ready conversion of `rgb`, built worker-side — the UI
    /// thread's only job is `tex.set` (the RGB→RGBA expansion of a 4-11 MP
    /// frame at 2560/4096 was a 5-15 ms stall per accepted frame).
    pub(crate) after: egui::ColorImage,
    pub(crate) histogram: Vec<[f32; 4]>,
    pub(crate) clipping: Option<egui::ColorImage>,
    pub(crate) thumb: egui::ColorImage,
}

/// Coverage-cache identity. Local effect sliders (Exposure/Temp/Saturation/
/// color_gains) are intentionally absent: they change pixels INSIDE a mask,
/// never its coverage. A Range Mask includes its PREFIX reference recipe
/// (earlier masks applied — the engine's stacking rule) because its weight
/// is judged on those developed pixels.
#[derive(Clone, PartialEq)]
pub(crate) struct OverlayKey {
    pub(crate) base: usize,
    pub(crate) target: usize,
    pub(crate) mask: MaskGeometry,
    // Coverage inputs the combined weight adds (v0.22): the component list
    // (each geometry + mode) and the eye toggle — without them an added /
    // re-moded / muted shape reused the stale wash.
    pub(crate) components: Vec<autoshade::recipe::MaskComponent>,
    pub(crate) enabled: bool,
    pub(crate) range: Option<RangeMask>,
    pub(crate) amount: f32,
    pub(crate) inverted: bool,
    pub(crate) reference_recipe: Option<EditRecipe>,
    pub(crate) straighten_deg: f32,
    pub(crate) lens_distortion: f32,
    // The coverage warp also depends on the PROFILE geometry toggles —
    // without them in the key, flipping profile Distortion/CA reused a wash
    // warped under the previous state (knot data only changes per photo,
    // which `base` already keys).
    pub(crate) profile_dist_on: bool,
    pub(crate) profile_ca_on: bool,
    // …and on the MANUAL CA pair (R25 B3), by VALUE. The two bools above say
    // whether the profile's CA is on, never how far anything is turned, and
    // the manual pair rides the same per-channel knots — so without this,
    // dragging Red/cyan re-warped the photo while the coverage wash kept the
    // warp it was built under.
    pub(crate) manual_ca: (f32, f32),
}

/// Messages from worker threads back to the UI. The large payloads are boxed so
/// the channel message stays small (clippy::large_enum_variant).
pub(crate) enum Msg {
    /// Decoded base + the photo's fresh start and the base part it was
    /// developed under (see [`OpenedBase`]).
    Opened(Box<anyhow::Result<OpenedBase>>),
    /// The source base re-developed under another base part (v1.17.0): the
    /// canvas recipe's camera profile or Look changed under a source-based
    /// card, and a preview develop cannot add a profile to pixels that lack
    /// it. Lands only while the photo and the wanted part still match.
    BaseRedeveloped {
        path: PathBuf,
        part: Box<autoshade::render::BasePart>,
        img: Box<anyhow::Result<Arc<image::DynamicImage>>>,
    },
    // (MasterLoaded carries the SPAWN-time edge and file stamp: the landing
    // used to stat and read preferences at completion time, filing the
    // pixels under an identity they never had — L12-2/L12-6.)
    /// A synchronous GUI develop used to block egui's update loop. Preview work
    /// now returns here from a single latest-wins worker.
    Developed(Box<anyhow::Result<PreviewDone>>),
    /// Carries the gen_epoch it was started under: Analyze is cancellable
    /// (abandon-only — the advisor has no cancel checkpoints, so ✕ unblocks
    /// the UI now and the network call dies at its stall/progress deadline),
    /// and a cancelled call's late result must be discarded, not installed
    /// and PERSISTED over whatever the user has since done. The third tuple
    /// element is the deterministic-rationale notes (L12#2B) the landing
    /// installs for draw-time localization.
    Analyzed(
        u64,
        Box<
            anyhow::Result<(
                EditRecipe,
                autoshade::advisor::Verdict,
                Vec<autoshade::rationale::Note>,
            )>,
        >,
    ),
    Exported(anyhow::Result<ExportOutcome>),
    /// A folder scan finished: (folder, sorted source paths).
    Folder(Box<anyhow::Result<(PathBuf, Vec<PathBuf>, usize)>>),
    /// A gallery thumbnail decoded. `generation` tags the folder generation so a
    /// folder switch can't insert a stale thumbnail under a reused index.
    Thumb { generation: u64, idx: usize, img: Box<anyhow::Result<image::DynamicImage>> },
    /// A cold-restored variant's retouched MASTER decoded off-thread. The
    /// (photo, origin) pair is the install identity — index-free, so a card
    /// deleted or reordered while the decode ran discards the late pixels
    /// instead of installing them into whatever now sits at the old index.
    MasterLoaded {
        photo: PathBuf,
        origin: PathBuf,
        /// SPAWN-time identity (L12-2/L12-6): the landing files the pixels
        /// under the edge + stamp the decode actually ran with, never under
        /// completion-time preferences or a completion-time stat.
        edge: u32,
        stamp: FileStamp,
        img: Box<anyhow::Result<image::DynamicImage>>,
    },
    /// A generative-fill / heal / clone / reimagine result — see [`RetouchDone`].
    /// The `u64` is the cancel epoch the task was started under: Cancel bumps
    /// the app's epoch, so a cancelled worker's late result arrives with a
    /// stale epoch and is discarded instead of mutating the canvas.
    Retouched(u64, Box<RetouchDone>),
    /// A liveness line from a running generative worker (partial-image
    /// heartbeats, negotiation notes) — mirrored into the status bar. Carries
    /// the worker's cancel epoch: an abandoned task's heartbeat must not
    /// overwrite the status of the task the user started after cancelling.
    Progress(u64, String),
    /// AI segmentation finished: (mask display name, grayscale raster path,
    /// backend label) — attached to the recipe as a `MaskGeometry::Bitmap`
    /// local mask. The third field is `segment::SegmentReport::backend`, the
    /// name the sidecar printed for the model that actually answered; it is
    /// shown and then dropped, never persisted, because it describes the RUN
    /// and the recipe is data. Empty when the sidecar did not say.
    Segmented(anyhow::Result<(String, PathBuf, String)>),
    /// The depth sidecar finished (2026-10-13): the claimed depth PNG for the
    /// AI lens blur, or why not.
    DepthEstimated(anyhow::Result<PathBuf>),
    /// The open card's Lightroom AI masks were re-derived on this computer
    /// (`persist.rs` `start_ai_mask_resolve`): the photo and card they were
    /// cut for, the recipe with the alphas found, and what the segmenter did.
    AiMasksResolved(Box<(PathBuf, String, EditRecipe, autoshade::pipeline::DevelopMasks, crate::persist::FaceOutcomes)>),
    /// Full-res mask refine finished — see [`MaskRefineOutcome`].
    MaskRefined(anyhow::Result<MaskRefineOutcome>),
    /// Batch render advanced: `done` of `total` photos finished (ok or err).
    BatchProgress { done: usize, total: usize },
    /// Reverse-fit finished — see [`FitOutcome`]: FACTS, not prose, so the
    /// landing renders the status in the language live at LANDING time
    /// (L12#4 — the old String was translated inside the spawn closure with
    /// the language captured at spawn, minutes stale by the time it landed).
    Fitted(Box<anyhow::Result<FitOutcome>>),
    /// Settings → "Import develops from an old ./out folder":
    /// (photo count, the folder) — rendered at landing (L12#4).
    LegacyImported(anyhow::Result<(usize, PathBuf)>),
    /// Style-prompt extraction finished: (the reusable prompt text, the
    /// typed fact of whether the .style.txt copy landed).
    Styled(Box<anyhow::Result<(String, StyleNote)>>),
    /// A `GET /models` fetch finished: the account's model ids (Settings
    /// pick-list). Tagged with the ROLE that asked, because the two roles can
    /// point at different endpoints with different catalogues — the analysis
    /// picker used to borrow the image endpoint's ids and offer nothing at all
    /// whenever the two base URLs differed. The `u64` is the catalogue
    /// generation captured at launch ([`ModelCatalogue::gen`]): a completion
    /// is installed only if nothing invalidated the catalogue while it flew.
    Models(ModelRole, u64, anyhow::Result<Vec<autoshade::openai_models::ModelInfo>>),
    /// A batch recipe paste finished — counts and per-photo details,
    /// rendered at landing (L12#4).
    Pasted(anyhow::Result<PasteOutcome>),
    /// A reference-library STATUS read finished (R23-2).
    StyleInfo(Box<autoshade::library::Summary>),
    /// The running reference-library build advanced: `done` of `total` photos
    /// in `stage`. Typed counts, worded at landing (L12#4) — same shape as
    /// [`Msg::BatchProgress`].
    StyleBuildProgress { stage: autoshade::library::BuildStage, done: usize, total: usize },
    /// A reference-library build finished — see [`StyleBuildOutcome`].
    StyleBuilt(Box<StyleBuildOutcome>),
    /// Normal's reference folder is being read into its own library
    /// (`panels/normal.rs`): `done` of `total` photos.
    OneClickBuildProgress { done: usize, total: usize },
    /// …and finished: how many photos the library holds, or why it failed.
    /// The landing hands over to the analysis (`on_one_click_built`).
    OneClickBuilt(Result<usize, String>),
    /// The managed Python environment's install printed a line (uv's stderr).
    PythonEnvLine(String),
    /// …and finished: the environment's interpreter, or why it failed.
    PythonEnvReady(anyhow::Result<String>),
    /// The Portraits panel's find step finished (`panels/portrait.rs`): the
    /// subject matte and the faces, as facts for the landing.
    PortraitFound(Box<crate::panels::portrait::PortraitFind>),
}

/// What building the reference library did — FACTS, not prose (L12#4): the
/// build runs for minutes, so its sentence is composed at LANDING in the
/// language live then, never with the language captured at spawn.
pub(crate) enum StyleBuildOutcome {
    /// Built and published: how many finished photos, from where, and how
    /// many carry look phrases (0 when the describe pass was off).
    Saved { total: usize, dir: PathBuf, phrased: usize },
    /// The build or the publish failed; the previous library is untouched.
    Failed { err: String },
}

/// What the full-resolution mask refine did — FACTS, not prose (L12#4, the
/// same rule [`FitOutcome`] follows): the worker runs for as long as a 61 MP
/// develop takes, so the sentence is composed at LANDING in the language live
/// then, never with the language captured at spawn.
pub(crate) enum MaskRefineOutcome {
    /// (the initiating mask index, the stored raster reference the job started
    /// from, the refined raster's path). Landing validates index AND reference
    /// together — the list may have changed mid-flight, and a bare path search
    /// could repoint the WRONG mask when two masks legitimately reference one
    /// raster (Codex R9-1).
    Refined(usize, String, PathBuf),
    /// A raster at the guide's resolution would not be loadable again NEXT TO
    /// this recipe's other active rasters (`render::mask_raster_write_fits_budget`
    /// — the aggregate budget the render's own loader charges, not this file
    /// alone). Refused with NOTHING written; the dimensions ride along because
    /// the refusal has to say which source it is talking about.
    OverBudget { w: u32, h: u32 },
}

pub(crate) use crate::logic::fit::FitNote;


pub(crate) use crate::logic::fit::DeepFitOutcome;


/// What the reverse-fit's INFORMATIONAL review should do at the end of a run,
/// given whatever the deep path left behind.
///
/// Pure, and named, because what it protects is a SPEND ceiling the surface
/// states as a number: 「deep」's tooltip promises at most two paid vision calls
/// per fit. The deep path's outcome used to be an `Option<Judgement>`, in which
/// "the review ran and FAILED" and "the deep path never ran" were the same
/// `None` — so two failed attempts bought a third, and reported the same
/// failure twice (R23 review LOW-6).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum FitReviewPlan {
    /// The deep path holds the verdict that describes what ships — reuse it,
    /// rather than paying for the same answer.
    Reuse,
    /// The deep path already tried and failed, and disclosed it once.
    Skip,
    /// Nobody has judged this recipe yet — buy the one informational call.
    Call,
}

impl FitReviewPlan {
    pub(crate) fn of<T, E>(deep: &Option<Result<T, E>>) -> Self {
        match deep {
            Some(Ok(_)) => Self::Reuse,
            Some(Err(_)) => Self::Skip,
            None => Self::Call,
        }
    }
}

/// The reverse-fit result: recipe + errors + typed notes. `persisted` is
/// false when the backup gate refused the store write — the ● baseline
/// must NOT be advanced then. `rationale_notes` are the fit rationale's
/// typed copy (L12#2B), installed for draw-time localization.
pub(crate) struct FitOutcome {
    pub(crate) recipe: EditRecipe,
    pub(crate) err_before: f32,
    pub(crate) err_after: f32,
    pub(crate) rationale_notes: Vec<autoshade::rationale::Note>,
    pub(crate) status: Vec<FitNote>,
    pub(crate) persisted: bool,
    /// The negative's in-place master the fit was solved on and the persist
    /// linked (`AutoShadeApp::negative_origin` at the click) — the ◭ card
    /// lands hanging off it, so what the fit measured, what the canvas shows
    /// and what `pixels.json` says are one file. `None` = the loaded photo.
    pub(crate) negative: Option<PathBuf>,
}

/// Default endpoint for the image-role OAuth preset: a local Codex bridge
/// (e.g. CLIProxyAPI) that replays a ChatGPT-subscription OAuth token as an
/// OpenAI-compatible API on loopback. Only a default — the field stays editable
/// so a custom host/port works too.
pub(crate) const CODEX_BRIDGE_URL: &str = "http://127.0.0.1:8317/v1";

/// The stock OpenAI endpoint. Used to recognise a stale default when flipping the
/// image role into OAuth mode, so we can swap in the bridge URL automatically.
pub(crate) const OPENAI_DEFAULT_URL: &str = "https://api.openai.com/v1";

/// Which role a `GET /models` probe was run for. The two roles have their own
/// endpoint, their own key, and therefore their own catalogue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ModelRole {
    /// The vision proposer + generative edits (`image_base_url`).
    Image,
    /// The verifier, in `api` mode (`analysis_base_url`).
    Analysis,
}

/// One endpoint's live pick-lists, as last fetched.
#[derive(Default)]
pub(crate) struct ModelCatalogue {
    /// Text/vision chat ids — the proposer and the API verifier.
    pub(crate) chat: Vec<String>,
    /// Image-generation ids. Only the image role has a use for these; the
    /// analysis catalogue leaves it empty.
    pub(crate) image_gen: Vec<String>,
    /// The reasoning tiers each id takes, for the ids whose listing SAID
    /// (`openai_models::ModelInfo::effort_options`): an empty list is a
    /// model that takes no tier. An id absent here is one the listing said
    /// nothing about, and its picker offers the fixed suggestions.
    pub(crate) efforts: std::collections::HashMap<String, Vec<String>>,
    /// Whether each id accepts an image part, for the ids whose listing said.
    pub(crate) vision: std::collections::HashMap<String, bool>,
    /// A `GET /models` worker is in flight for this role.
    pub(crate) fetching: bool,
    /// The endpoint these ids came FROM. Editing the Base/Bridge URL (or the
    /// provider auto-swap rewriting it) makes them ids of a different server —
    /// the pickers self-invalidate instead of offering stale ids.
    pub(crate) from_base: String,
    /// Validity stamp for in-flight fetches: bumped by every [`clear`], and
    /// captured by `fetch_models` into [`Msg::Models`]. A completion whose
    /// stamp no longer matches was fetched for a world (key, endpoint) that a
    /// mid-flight edit invalidated — installing it would resurrect the OLD
    /// credential's ids under the new one's name. Same claim/installed shape
    /// as serve.rs's folder generations.
    ///
    /// [`clear`]: ModelCatalogue::clear
    pub(crate) generation: u64,
    /// Fingerprint of the credential the ids were fetched WITH (availability
    /// is credential-dependent, not just URL-dependent). Compared on a
    /// Settings reopen, where another surface (the web panel) may have
    /// replaced the saved key since this list was fetched. See
    /// `util::key_fingerprint` — an in-memory staleness stamp, never
    /// persisted, never displayed.
    pub(crate) from_key: u64,
    /// This role's once-per-session convenience probe has been DISPATCHED.
    /// Consumed at dispatch, not at Settings-open: a role that only becomes
    /// eligible later in the session (key added, provider flipped to `api`)
    /// still gets its one auto-fetch, while an already-probed role never
    /// re-probes a metered endpoint on every reopen.
    pub(crate) autofetched: bool,
}

impl ModelCatalogue {
    pub(crate) fn is_empty(&self) -> bool {
        self.chat.is_empty() && self.image_gen.is_empty()
    }

    /// Forget everything fetched. Used when the endpoint or the credential
    /// changes — both decide which ids exist. Bumps `gen`: whatever is still
    /// in flight was fetched for the world this clear just discarded.
    pub(crate) fn clear(&mut self) {
        self.chat.clear();
        self.image_gen.clear();
        self.efforts.clear();
        self.vision.clear();
        self.generation += 1;
    }

    /// The tiers to offer for `id`: what its listing said, when it said
    /// (an empty list for a model that takes none), else `fallback`.
    pub(crate) fn tiers_for<'a>(&'a self, id: &str, fallback: &'a [&'a str]) -> Vec<&'a str> {
        match self.efforts.get(id.trim()) {
            Some(known) => known.iter().map(String::as_str).collect(),
            None => fallback.to_vec(),
        }
    }

    /// `Some(false)` when the listing said this id takes no image — the one
    /// answer worth a warning beside the vision picker.
    pub(crate) fn vision_of(&self, id: &str) -> Option<bool> {
        self.vision.get(id.trim()).copied()
    }
}

/// Reasoning-effort tiers offered by the pickers. Suggestions, not a closed
/// set: both pickers sit beside a free-text field, `config::effort` only
/// BOUNDS the value, and an endpoint that rejects a tier is negotiated down
/// (`the removed paid transport`, `claude::cli_rejected_effort`).
///
/// The Claude CLI's list is quoted from `claude --help` on this machine
/// (`--effort <level>  Effort level for the current session (low, medium,
/// high, xhigh, max)`, measured 2026-08-11). OpenAI-compatible endpoints
/// share the first three; anything else goes in the text field.
pub(crate) const EFFORT_TIERS_CLI: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
pub(crate) const EFFORT_TIERS_API: [&str; 3] = ["low", "medium", "high"];

/// The `claude` CLI's model ALIASES, from the same `--help` (`--model
/// <model>  … Provide an alias for the latest model (e.g. 'fable', 'opus', or
/// 'sonnet') …`). Used both as the OAuth picker's list and as the test for
/// "is this id a Claude alias" when the provider radio flips — a hardcoded
/// trio here used to rewrite a legitimate `fable` back to `opus`.
pub(crate) const CLAUDE_ALIASES: [&str; 4] = ["fable", "opus", "sonnet", "haiku"];

/// Editable buffers for the in-app Settings window. Key fields stay blank on
/// load and only overwrite the stored key when non-empty (the form never shows
/// an existing secret) — mirroring the web `/api/settings` contract.
#[derive(Default)]
pub(crate) struct SettingsForm {
    pub(crate) analysis_provider_api: bool, // false = OAuth (claude CLI), true = OpenAI-compatible API
    pub(crate) image_provider_oauth: bool,  // true = Codex bridge (ChatGPT sub), false = OpenAI-compatible API
    pub(crate) analysis_model: String,
    pub(crate) analysis_base_url: String,
    pub(crate) analysis_api_key: String,
    pub(crate) analysis_key_present: bool,
    pub(crate) analysis_effort: String,
    pub(crate) image_model: String,
    pub(crate) image_base_url: String,
    pub(crate) image_gen_model: String,
    pub(crate) image_api_key: String,
    pub(crate) image_key_present: bool,
    pub(crate) image_effort: String,
    /// `auto` / `responses` / `chat` — the request shape of the image role
    /// (`config::Config::image_api_family`); the picker writes the spelling
    /// the settings file stores.
    pub(crate) image_api_family: String,
    /// The DELIVERY ROOT as the settings FILE spells it (R24-5 M8) — blank
    /// means "the default `./out`", which is also how a blank SAVES (an
    /// explicitly emptied field silences an `AUTOSHADE_OUT_DIR` in the
    /// environment, the same rule the two effort fields follow). The panel
    /// echoes the RESOLVED absolute path beside it, since that is the fact a
    /// relative root leaves unanswered.
    pub(crate) out_dir: String,
    /// The PYTHON INTERPRETER as the settings FILE spells it (M1-3) — blank
    /// means "the platform default" (`config::default_python_bin`), and a
    /// blank SAVES as blank for the same reason the delivery root does: only
    /// an explicitly stored empty value can undo a path saved earlier.
    ///
    /// The panel echoes the RESOLVED spelling beside it, because an
    /// `AUTOSHADE_PYTHON` in the environment outranks this file and a user
    /// whose environment already answers would otherwise see an empty box.
    pub(crate) python_bin: String,
    /// ComfyUI (local generative backend) paths, as the settings FILE spells
    /// them. Blank = derive the default: the CLI from PATH, the workspace and
    /// models folder from the Comfy Desktop registry, and the managed input/
    /// output/temp folders under the app store (so generated pixels never land
    /// in ComfyUI's own output folder).
    pub(crate) comfy_cli: String,
    pub(crate) comfy_workspace: String,
    pub(crate) comfy_base_dir: String,
    pub(crate) comfy_reimagine_workflow: String,
    pub(crate) comfy_fill_workflow: String,
    pub(crate) status: String,
    /// Live pick-lists, one per endpoint (see [`ModelCatalogue`]). Each
    /// carries its own once-per-session auto-fetch guard: one global boolean
    /// here was consumed by the first Settings open even when NO role was
    /// eligible yet, so a key saved later in the session never got its probe.
    pub(crate) image_models: ModelCatalogue,
    pub(crate) analysis_models: ModelCatalogue,
}

/// One deferred action on a variant CARD (R24-5), performed by the single
/// owner `AutoShadeApp::dispatch_variant_action`.
///
/// Every one of these mutates the app while the drawing closures still borrow
/// it, so the widget code returns a value and the caller acts after the
/// layout closes — the strip's original five local `Option`s in one type. A
/// single value also keeps the arms mutually exclusive by construction: one
/// frame cannot switch a card and delete it.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum VariantAction {
    /// Show card `i` on the canvas (lossless — each card keeps its own
    /// base + recipe).
    Switch(usize),
    /// Drop card `i`. Arm-then-confirm lives in `delete_variant` itself.
    Delete(usize),
    /// Copy card `i`'s develop onto the ▣ Original card.
    Apply(usize),
    /// Open the rename box for the card with this ID (never an index: an
    /// async push renumbers the strip under a stale index).
    Rename(String),
    /// Snapshot the LIVE canvas as the next numbered version.
    SaveVersion,
}

/// Which surface is drawing a card's actions (R24-5). Both draw the same
/// buttons through one owner; they differ only in which headless test seam
/// records the result, and in the strip's exclusive right to the rename box —
/// two `TextEdit`s over one buffer would fight for the caret every frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum VariantSurface {
    /// The bottom variant strip.
    Strip,
    /// The Versions section's edit-state list.
    List,
}

/// Pick radius (px) for on-image mask knobs — matches the crop handles' feel.
pub(crate) const HANDLE_HIT: f32 = 12.0;

/// What the next image drag defines (armed by the mask panel's buttons):
/// a brand-new mask, a redraw of mask `i`'s base area, or a new COMPONENT
/// appended to mask `i` with the given combine mode.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum PlaceTarget {
    NewMask,
    Redraw(usize),
    Component(usize, autoshade::recipe::MaskCombine),
}

/// Curve-editor channels: label + PLOT draw colour, indexed by `curve_channel`
/// (0 = master, then R/G/B — the recipe's tone/red/green/blue_curve fields).
/// The plot colours are theme-independent — the curve square stays dark on
/// both themes (see [`ThemeColors`]); the PICKER LABELS above the plot sit on
/// panel chrome and take their colours from `ThemeColors::curve_labels`.
// Tone-curve channel picker labels — skeleton keys, localized with `tr` at the
// render site (curve_editor). Colours are the on-curve accent, not localized.
pub(crate) const CURVE_CHANNELS: [(&str, egui::Color32); 4] = [
    ("Master", egui::Color32::from_gray(225)),
    ("Red", egui::Color32::from_rgb(235, 90, 90)),
    ("Green", egui::Color32::from_rgb(90, 205, 90)),
    ("Blue", egui::Color32::from_rgb(90, 130, 240)),
];

/// WHICH four curves the editor is pointed at. The recipe grew a second set in
/// R25 P6 — every mask carries its own master + RGB point curves
/// (`crs:MainCurve` / `RedCurve` / `GreenCurve` / `BlueCurve`) — and the
/// editor is one widget serving both, so the target is a parameter rather than
/// a second copy of 200 lines of plot and gesture code.
///
/// `curve_channel` (which of the four) stays a shared UI preference: it is the
/// picker's position, and Lightroom likewise has one channel picker. The
/// DRAG, by contrast, is scoped to the target it started on (`curve_drag`
/// carries this value), because two editors can be laid out in the same frame
/// and a drag begun on one must not move a point in the other.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CurveTarget {
    /// The recipe's own global curves (`EditRecipe::tone_curve` …).
    Global,
    /// One mask's curves, by index into `EditRecipe::masks`.
    Mask(usize),
}

/// The lens-geometry half of the view mapping: the in-camera profile (the
/// composed map needs its distortion knots) plus the manual amount. Cloned
/// out of `geom_ctx` per gesture — a few 16-float Vecs, noise next to any
/// develop tick.
#[derive(Clone, Default)]
pub(crate) struct LensArg {
    pub(crate) profile: autoshade::recipe::LensProfile,
    pub(crate) amount: f32,
}

/// Slider feel classes — Lightroom's stepping grammar. `Int`: whole-number
/// domains (the ±100 family, 0..150, hue °); `Fine`: sub-unit precision
/// (Straighten °); `Frac`: small fractional domains (EV, 0..1 amounts);
/// `LogK`: the log-scaled Temp (K) track.
#[derive(Copy, Clone)]
pub(crate) enum SliderFeel {
    Int,
    Fine,
    Frac,
    LogK,
}

/// One undo/redo step: the recipe plus the active variant's pixel identity.
/// A baked pixel retouch (heal / clone / denoise) swaps the variant's
/// base + origin outside the recipe — carrying them here makes Ctrl+Z walk
/// back through retouches too. History is per-variant (reset on switch), so a
/// step's pixels always belong to the variant it was recorded on; the base is
/// compared by `Arc::ptr_eq`, never by pixels.
#[derive(Clone, Default)]
pub(crate) struct UndoStep {
    pub(crate) recipe: EditRecipe,
    pub(crate) base: Option<Arc<image::DynamicImage>>,
    pub(crate) origin: Option<PathBuf>,
    /// The card's "Keep skin natural" switch on this step (Portraits): the
    /// switch-on that brings values back within the limits is a step, and
    /// redoing it has to turn the switch back on with them. A flip that
    /// changes no value records no step and amends the head instead
    /// (`commit_now`). None: no photo was open.
    pub(crate) keep_skin: Option<bool>,
}

impl UndoStep {
    /// Same pixel identity? (Arc pointer + origin path — cheap, exact.)
    pub(crate) fn same_pixels(&self, other: &UndoStep) -> bool {
        let base_eq = match (&self.base, &other.base) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        // Master identity, not string identity: this was the one comparison
        // the batch-80 sweep left on `==`, which made its own "EVERY" false.
        // Benign today — a step's base and origin are always replaced
        // together, so no same-Arc / two-spellings pair can reach here — and
        // `same_master` tries plain equality first, so the common path is
        // unchanged.
        base_eq && same_master_opt(self.origin.as_deref(), other.origin.as_deref())
    }
}

/// One photo the quit dialog would save. Was a positional 4-tuple with a
/// nested pair — six values threaded by position (round-12 结构 cluster).
pub(crate) struct PendingSave {
    pub(crate) photo: PathBuf,
    /// The canvas recipe Save-all writes, exactly like Ctrl+S.
    pub(crate) recipe: EditRecipe,
    /// The canvas's baked pixel identity when one exists:
    /// (master origin, is_generated) — persists the master link.
    pub(crate) pixels: Option<(PathBuf, bool)>,
    /// The photo's strip record (background variants + active kind/position)
    /// when the strip is non-trivial — Save-all persists the WHOLE strip.
    pub(crate) strip: Option<autoshade::store::VariantsRecord>,
}

/// The canvas snapshot navigation stashes for a photo with unsaved work:
/// the recipe PLUS the active variant's pixel identity. A baked in-place
/// retouch is unsaved work too — restoring the recipe alone silently dropped
/// the healed pixels when you navigated away and back. `base` is the
/// preview-res Arc (refcount bump, no copy); `origin` the full-res master.
pub(crate) struct StashEntry {
    pub(crate) recipe: EditRecipe,
    pub(crate) base: Option<Arc<image::DynamicImage>>,
    pub(crate) origin: Option<PathBuf>,
    /// The ACTIVE variant's kind — a plain `generated: bool`
    /// here collapsed `Fitted` onto `Original`, so a 「◭ 反推」 card came back
    /// from navigation renamed 「▣ 原片」 (the strip's only rename bug that
    /// needed no disk at all).
    pub(crate) kind: VariantKind,
    /// The ACTIVE variant's identity + user name (R24-2), carried for the
    /// same reason its `kind` is: a nav round-trip that re-minted the id
    /// would orphan every version snapshot taken from this card.
    pub(crate) id: String,
    pub(crate) name: Option<String>,
    /// The rest of the variant strip (everything but the active variant),
    /// so navigation restores the WHOLE session: a dirty BACKGROUND variant
    /// used to die silently on nav (H4) — the quit dialog discloses that
    /// loss honestly because quitting has nothing to restore into, but
    /// navigation does: this session's stash.
    pub(crate) others: Vec<StashedVariant>,
    /// Where the active variant sat in the strip (`others` = strip minus it).
    pub(crate) active_pos: usize,
}

/// One background variant in a [`StashEntry`] — thumb-less (textures do not
/// survive a photo switch; they rebuild on the first develop after restore).
pub(crate) struct StashedVariant {
    pub(crate) kind: VariantKind,
    /// Identity + user name travel with the card (R24-2): the stash is one
    /// of the six hops a variant name has to survive, and a re-minted id on
    /// the way back would break every version attributed to this card.
    pub(crate) id: String,
    pub(crate) name: Option<String>,
    pub(crate) recipe: EditRecipe,
    pub(crate) base: Option<Arc<image::DynamicImage>>,
    pub(crate) origin: Option<PathBuf>,
}

/// One rendition in the variant strip — a Lightroom-style virtual copy /
/// Capture One variant, NOT a compositing layer (variants never blend; you
/// switch between them losslessly). A variant is fully defined by its base
/// pixels + its develop recipe.
pub(crate) struct Variant {
    pub(crate) kind: VariantKind,
    /// Stable, opaque identity for THIS card inside THIS photo (R24-2).
    /// Persisted in `variants.json`, so it survives a reopen — that is what
    /// lets a version snapshot say which variant it came from after the
    /// session that took it is gone. Never shown; never parsed. The empty
    /// string is the honest "no identity recorded" value (a legacy strip
    /// record, a test fixture): it matches nothing, rather than matching
    /// every other id-less card.
    pub(crate) id: String,
    /// The user's own name for this rendition. `None` ⇒ the strip shows the
    /// kind label alone. Deliberately NOT part of `EditRecipe`: recipe.json
    /// is the cross-surface develop contract, its bare `PartialEq` is the
    /// backup gate's "already preserved" witness, and `dirty_vs` compares it
    /// field-by-field — a name in there would make renaming a card look like
    /// an edit to every one of them.
    pub(crate) name: Option<String>,
    /// This variant's develop. The ACTIVE variant's recipe is mirrored in
    /// `AutoShadeApp::recipe` (the live working copy the sliders edit); it is
    /// saved back here when you switch away.
    pub(crate) recipe: EditRecipe,
    /// Base pixels this variant develops from. Arc makes variant switches and
    /// background preview dispatch O(1); pixels remain immutable.
    /// `None` ⇒ the shared source neutral (`AutoShadeApp::source_preview`).
    pub(crate) base: Option<Arc<image::DynamicImage>>,
    /// The pixel master behind a raster variant (the reimagine PNG) — the
    /// reverse-fit target and the full-res export source. `None` for
    /// source-based variants.
    pub(crate) origin: Option<PathBuf>,
    /// Small developed thumbnail for the strip (rebuilt for the active variant
    /// on every develop; built once for the others when created / left).
    pub(crate) thumb: Option<egui::TextureHandle>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum VariantKind {
    Original,  // 原片 — the loaded RAW / image, your develop
    Generated, // AI 生成 — a whole-frame gpt-image restyle (look in the pixels)
    Fitted,    // 反推 — the generated look solved back into an editable recipe
    Edited,    // 生图编辑 — the user's own develop over a Generated card's pixels
    Denoised,  // 去噪原片 — the negative AI-denoised into a new master (2026-09-15)
    Stacked,   // 堆栈 — several frames merged into a new master (v1.5.0 Track S)
}

impl VariantKind {
    /// Strip label (icon + English key; localised via `tr` at the render site).
    pub(crate) fn label(self) -> &'static str {
        match self {
            VariantKind::Original => "▣ Original",
            VariantKind::Generated => "✨ AI generated",
            VariantKind::Fitted => "◭ Reverse-fit",
            VariantKind::Edited => "✎ Edited AI image",
            VariantKind::Denoised => "◈ Denoised negative",
            VariantKind::Stacked => "▦ Stacked",
        }
    }

    /// The `variants.json` spelling — persisted, so these strings are a
    /// FORMAT, not display text: never localise or rename them.
    pub(crate) fn store_str(self) -> &'static str {
        match self {
            VariantKind::Original => "original",
            VariantKind::Generated => "generated",
            VariantKind::Fitted => "fitted",
            VariantKind::Edited => "edited",
            VariantKind::Denoised => "denoised",
            VariantKind::Stacked => "stacked",
        }
    }

    pub(crate) fn from_store_str(s: &str) -> Option<Self> {
        match s {
            "original" => Some(VariantKind::Original),
            "generated" => Some(VariantKind::Generated),
            "fitted" => Some(VariantKind::Fitted),
            "edited" => Some(VariantKind::Edited),
            "denoised" => Some(VariantKind::Denoised),
            "stacked" => Some(VariantKind::Stacked),
            _ => None,
        }
    }

    /// The variant taxonomy's ONE binary (R24-1). A photo is *one negative +
    /// N variants + one version history*; every variant is either
    ///
    /// * SOURCE-BASED (`Original`, `Fitted`, `Denoised`, `Stacked`) — the look lives in
    ///   the develop recipe over the photo's own pixels, so it re-renders at
    ///   any resolution, projects to XMP, and carries the camera calibration
    ///   the base-curve estimator repairs. A `Denoised` card (2026-09-15) is
    ///   the negative itself, AI-denoised on its sensor mosaic into a new
    ///   master: its `origin` is that master, its recipe is the develop it was
    ///   made from, and while it exists it is the negative the reverse-fit and
    ///   the reimagine read (`AutoShadeApp::negative_origin`); or
    /// * ON AI PIXELS (`Generated`, `Edited`) — the card develops a
    ///   generated raster. A `Generated` card is the raster itself and is
    ///   IMMUTABLE (2026-09-13, user decision): its recipe is neutral by
    ///   invariant, and an edit made on it continues on an `Edited` card —
    ///   the user's own develop over that same raster
    ///   (`AutoShadeApp::fork_edited_card`). Both carry NO calibration (the
    ///   raster already has the camera look, the lens geometry and a baked
    ///   white balance), neither projects to XMP (no sidecar reproduces AI
    ///   pixels), and only a reverse-fit turns the raster's look back into
    ///   parameters on the negative.
    ///
    /// `origin.is_some()` ("this card hangs off a baked master") is an
    /// ORTHOGONAL second attribute, not this axis: an in-place retouch master
    /// hangs off an *Original* — and off every *Fitted* card solved after it,
    /// which inherits the negative's master (`AutoShadeApp::negative_origin`,
    /// 2026-09-13) and is source-based all the same. The exhaustive match is
    /// the point — a fifth kind must declare which side it lands on instead
    /// of inheriting `!= Generated`.
    ///
    /// The `pixels.json` two-valued `generated` flag IS this axis since the
    /// taxonomy grew its fourth kind: it records that the develop's master
    /// is an AI raster, which an `Edited` card shares with the `Generated`
    /// card it forked from — [`VariantKind::on_ai_pixels`] spells it at every
    /// flag site, so the flag cannot drift from the taxonomy.
    ///
    /// Deliberately NOT collected here (checked site by site, R24-1):
    /// * `fit_target` (canvas.rs) — a policy about which raster the
    ///   reverse-fit defaults to (the `Generated` card's, never an `Edited`
    ///   card's: the fit reads pixels, and the edits are sliders the user
    ///   already has), not a claim about this axis;
    /// * the `== Original` / `!= Original` branches (the strip's ✕ guard,
    ///   the strip-triviality judgements) — a different axis entirely;
    /// * the `== Generated` checks that guard the immutability rule itself
    ///   (the fork, `save_version`'s refusal, the door's normalisation) —
    ///   they ask about the one kind, not the side.
    pub(crate) fn is_source_based(self) -> bool {
        match self {
            VariantKind::Original
            | VariantKind::Fitted
            | VariantKind::Denoised
            | VariantKind::Stacked => true,
            VariantKind::Generated | VariantKind::Edited => false,
        }
    }

    /// Is this card the negative REMADE — a new master of the photograph
    /// itself, standing beside the one it was made from?
    ///
    /// Two kinds are, and for the same reason: a ◈ denoise and a ▦ stack both
    /// produce a new full-resolution master of the SAME scene, which is what
    /// every later stage should read as the negative
    /// (`AutoShadeApp::negative_origin`). A ◭ reverse-fit is source-based too
    /// and is NOT one of these — it carries a recipe, not pixels of its own.
    pub(crate) fn is_remade_negative(self) -> bool {
        matches!(self, VariantKind::Denoised | VariantKind::Stacked)
    }

    /// The other side of [`VariantKind::is_source_based`], named for the
    /// sites that feed the `pixels.json` flag and the calibration strip.
    pub(crate) fn on_ai_pixels(self) -> bool {
        !self.is_source_based()
    }
}

/// The fixed id of a photo's base negative card. A photo has exactly one
/// Original, and the common photo (one card, no `variants.json` at all —
/// the strip record is deliberately not written for a trivial strip) must
/// still keep a STABLE identity across reopens, or every version snapshot
/// taken from it would come back unattributed. Minted ids are for the cards
/// that only exist because something created them.
pub(crate) const ORIGINAL_VARIANT_ID: &str = "original";

/// Is this strip TRIVIAL — fully described by `recipe.json` + `pixels.json`,
/// so a `variants.json` beside them would be pure sidecar noise?
///
/// ONE owner for the judgement (R24-3): `current_strip_record` (the live
/// strip) and `stash_strip_record` (a navigated-away photo's) both answer it,
/// and both must answer the same or a name typed on one path survives a save
/// while the same name on the other does not.
///
/// A lone base negative is trivial only while it carries nothing the record
/// would be the ONLY home for:
/// * a NAME — renameable cards (R24-3) put user text on the one card that
///   used to need no sidecar, and dropping the record would silently discard
///   it;
/// * an IDENTITY other than the fixed [`ORIGINAL_VARIANT_ID`] — a card whose
///   id was minted (it came back from a record, or from a strip that had
///   more cards) is what this photo's version snapshots point AT, so losing
///   it un-attributes them for good. The fixed id needs no record: every
///   reader reconstructs it.
pub(crate) fn strip_is_trivial(
    kind: VariantKind,
    id: &str,
    name: Option<&str>,
    others: usize,
) -> bool {
    others == 0
        && kind == VariantKind::Original
        && name.is_none()
        && (id.is_empty() || id == ORIGINAL_VARIANT_ID)
}

/// A card's id as the strip record spells it: an EMPTY id is "no identity
/// recorded", which the record states by omitting the field rather than by
/// writing `""` (a stored empty string would read back as a real id that
/// happens to equal every other card's).
pub(crate) fn variant_id_field(id: &str) -> Option<String> {
    (!id.is_empty()).then(|| id.to_string())
}

/// The inverse, on the way back in: a record that carries no id gets one
/// MINTED (R24-2). Legacy `variants.json` files predate the field, and a
/// card that stayed id-less could never be named as a version's source.
pub(crate) fn variant_id_or_mint(id: Option<&String>) -> String {
    match id {
        Some(s) if !s.is_empty() => s.clone(),
        _ => new_variant_id(),
    }
}

/// Mint an opaque variant id, unique across processes and calls: pid,
/// wall-clock nanos and a process-local counter. Opaque on purpose — nothing
/// parses it back, so the shape can change without a format bump. (Two
/// AutoShade processes editing the same photo mint from different pids; two
/// cards in one process differ in the counter even inside one nanosecond.)
pub(crate) fn new_variant_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{:x}-{nanos:x}-{seq:x}", std::process::id())
}

/// The two mask geometries a user can place by dragging.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum MaskKind {
    Linear,
    Radial,
}

/// Export colour-space display names (indices = `exp_space`). Shared by the
/// Export section and the toolbar buttons' delivery-summary hover.
pub(crate) const EXPORT_SPACES: [&str; 3] =
    ["sRGB (universal)", "Display P3 (wide-gamut screens)", "Adobe RGB (print)"];

/// Crop aspect presets. `None` = free; `Some(r)` = width/height in PIXELS
/// (0.0 is the "original" sentinel, resolved against the photo at drag time).
// Display names are English skeleton keys (localised via `tr` at the render
// site); the ratio values are language-neutral. "1:1"…"9:16" have no ZH entry
// and fall back to themselves in both languages.
pub(crate) const CROP_ASPECTS: [(&str, Option<f32>); 11] = [
    ("Free", None),
    ("Original", Some(0.0)),
    ("1:1", Some(1.0)),
    ("3:2", Some(1.5)),
    ("2:3", Some(2.0 / 3.0)),
    ("4:3", Some(4.0 / 3.0)),
    ("3:4", Some(3.0 / 4.0)),
    ("5:4", Some(1.25)),
    ("4:5", Some(0.8)),
    ("16:9", Some(16.0 / 9.0)),
    ("9:16", Some(9.0 / 16.0)),
];

/// Screen ⇄ full-frame-normalized mapping through the visible uv window
/// (committed crop × zoom/pan). ALL image-space state — regions, mask
/// geometry, crop, the paint canvas — lives in full-frame normalized
/// coordinates, so every interaction handler maps through this one struct and
/// zoom/crop can never silently break any of them.
#[derive(Clone, Copy)]
pub(crate) struct ViewXform {
    pub(crate) rect: egui::Rect, // where the (visible part of the) image is drawn
    pub(crate) uv: egui::Rect,   // which full-frame region is visible (texture uv)
}

impl ViewXform {
    pub(crate) fn to_norm(self, p: egui::Pos2) -> (f32, f32) {
        // .max(1e-6), the same floor to_screen uses (L13-11): max(1.0)
        // REPLACED a sub-point rect dimension with a full point, compressing
        // the inverse map's whole axis — an extreme-panorama edge (>600:1)
        // put every crop/mask/brush gesture at the wrong coordinate.
        let fx = ((p.x - self.rect.min.x) / self.rect.width().max(1e-6)).clamp(0.0, 1.0);
        let fy = ((p.y - self.rect.min.y) / self.rect.height().max(1e-6)).clamp(0.0, 1.0);
        (self.uv.min.x + fx * self.uv.width(), self.uv.min.y + fy * self.uv.height())
    }

    pub(crate) fn to_screen(self, nx: f32, ny: f32) -> egui::Pos2 {
        egui::pos2(
            self.rect.min.x
                + (nx - self.uv.min.x) / self.uv.width().max(1e-6) * self.rect.width(),
            self.rect.min.y
                + (ny - self.uv.min.y) / self.uv.height().max(1e-6) * self.rect.height(),
        )
    }
}

/// A file's identity for cache purposes: mtime + size (size joins mtime like
/// the thumbnail key — a same-mtime replacement is otherwise indistinguishable
/// from the cached copy).
pub(crate) type FileStamp = Option<(std::time::SystemTime, u64)>;

/// The BAKED-PIXEL identity a cached base was decoded FOR: which master the
/// develop resolved to, whether it was AI-generated, and that master's own
/// stamp. `None` = a parametric-only develop (no pixels.json).
pub(crate) type PixelIdentity = Option<(PathBuf, bool, FileStamp)>;

/// Decoded-base LRU key: source path + requested preview edge + the source's
/// stamp + the baked-pixel identity.
///
/// The last component is not optional bookkeeping. A develop's pixels can
/// change while the SOURCE file never moves: reverse-fit clears pixels.json,
/// a retouch writes a new master, a save drops the link. `forget_open_base()`
/// exists to evict eagerly at those moments, but three separate review passes
/// found the same class of bug — one call site that forgot it (reverse-fit),
/// after which navigating away and back within the LRU resurrected the old
/// generated master and rendered the new recipe on pixels it was never
/// computed against. Putting the identity IN the key makes a stale hit
/// impossible instead of merely unlikely.
pub(crate) type BaseCacheKey = (PathBuf, u32, FileStamp, PixelIdentity);

/// A persisted baked master restored at open (store `pixels.json`): its
/// preview-res pixels, the full-res path, and whether it is an AI-generated
/// rendition (restores as a Generated variant — no parametric XMP).
pub(crate) type BakedBase = (Arc<image::DynamicImage>, PathBuf, bool);

/// A decoded preview base + the photo's FRESH start and the base part the
/// pixels were developed under (v1.17.0: `pipeline::CanvasStart` — Lightroom's
/// Adobe Color canvas where this machine can render it, else the
/// camera-matched base curve — and `render::BasePart`, which the canvas
/// compares against its recipe's own) + its in-camera lens
/// profile (`pipeline::fresh_lens_profile`; default = none available) + its
/// as-shot WB anchor (`render::as_shot_wb`; None = unknown → 5500 K).
/// The sixth element is the decode's IDENTITY, captured in the worker BEFORE
/// any read (working edge + source stamp + baked-pixel identity):
/// `remember_base` files the entry under it, never under completion-time
/// state — the curve-memo rule.
pub(crate) type OpenedBase = (
    Arc<image::DynamicImage>,
    (autoshade::pipeline::CanvasStart, autoshade::render::BasePart),
    autoshade::recipe::LensProfile,
    Option<(f32, f32)>,
    Option<BakedBase>,
    (u32, FileStamp, PixelIdentity),
    // A baked master that FAILED to decode (the canvas degraded to the
    // un-retouched source): the error detail, surfaced as a landing toast —
    // stderr is invisible in the windowed build (L11-3). Never cached
    // (remember_base stores None), so a revisit does not re-toast.
    Option<String>,
);

/// How many decoded preview bases to keep for instant photo revisits (~3.3 MB
/// each at the default 1280 edge; culling flips between neighbours constantly).
pub(crate) const BASE_CACHE_CAP: usize = 4;

/// How many decoded cold-variant masters to keep, so revisiting a strip card
/// is instant instead of a fresh multi-second decode. Entry-count, not bytes
/// (the same weakness `BASE_CACHE_CAP` carries): ~3.3 MB each at the default
/// 1280 edge but ~33 MB each at 4096, so the resident worst case tracks the
/// px preference.
pub(crate) const MASTER_CACHE_CAP: usize = 3;

impl AiPrefs {
    /// The Analysis fold's Strength dial as the typed axis the develop request
    /// reads (R23-3). Nothing else reads it.
    pub(crate) fn analysis_strength(&self) -> autoshade::recipe::GradeStrength {
        autoshade::recipe::GradeStrength::new(self.grade_strength)
    }

    /// The Reverse-fit fold's OWN Strength dial as the fit's honesty budget
    /// (F1). Separate from [`Self::analysis_strength`] on purpose — user
    /// decision 2026-09-12: the two functions live in two folds, so each has
    /// its own control and neither reading reaches the other. Through v1.3.1
    /// both read one `panel_strength()`, and the fit's budget was a dial two
    /// folds above the fit's own row.
    pub(crate) fn fit_strength(&self) -> autoshade::recipe::GradeStrength {
        autoshade::recipe::GradeStrength::new(self.fit_strength)
    }
}

#[cfg(test)]
mod pixel_identity_tests {
    use super::*;

    fn step(origin: &str, base: Option<Arc<image::DynamicImage>>) -> UndoStep {
        UndoStep {
            recipe: EditRecipe::default(),
            base,
            origin: Some(PathBuf::from(origin)),
            keep_skin: None,
        }
    }

    #[test]
    fn one_master_written_two_ways_is_one_pixel_identity() {
        let base = Some(Arc::new(image::DynamicImage::new_rgb8(4, 3)));
        let rel = step("out/_spelling.png", base.clone());
        let abs = step(&std::path::absolute("out/_spelling.png").unwrap().display().to_string(), base);
        assert!(rel.same_pixels(&abs), "relative and absolute spellings of one master match");
    }

    #[test]
    fn a_different_master_or_base_is_not_the_same_pixel_identity() {
        let rel = step("out/_spelling.png", Some(Arc::new(image::DynamicImage::new_rgb8(4, 3))));
        let other_master = step("out/_spelling-2.png", rel.base.clone());
        assert!(!rel.same_pixels(&other_master), "a different master differs even on one base");
        let other_base = step("out/_spelling.png", Some(Arc::new(image::DynamicImage::new_rgb8(4, 3))));
        assert!(!rel.same_pixels(&other_base), "a different Arc base differs even at one path");
    }

    #[test]
    fn no_base_on_either_side_matches_on_the_master() {
        let a = step("out/_spelling.png", None);
        let b = step("out/_spelling.png", None);
        assert!(a.same_pixels(&b));
        let c = step("out/_other.png", None);
        assert!(!a.same_pixels(&c));
    }
}
