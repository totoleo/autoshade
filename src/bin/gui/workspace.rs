//! The client-only workspace state: everything the renderer remembers that is
//! NOT the photo's edit and must never be persisted — folds, tabs, hovers,
//! windows, the gallery scan, tool arming, transient status.
//!
//! It lives in its own aggregate so the composition root is a wiring point
//! rather than a drawer of view fields. Its fields are private: reads go
//! through the value/`&` accessors, writes through `_mut` accessors, the scalar
//! and text doors (`set_status`, `set_busy`, …), and the two multi-field
//! methods a single draw path needs (`paint_upload_parts`, `brush_parts`).

use crate::*;
use crate::normal_actions::folder_name;
use crate::panels::retouch::{BRUSH_R_MAX, BRUSH_R_MIN};
use crate::export::export_dest_dir;
use crate::masks::export_png_of;
use crate::panels::normal::NormalLine;
use crate::panels::portrait::PortraitLine;

pub(crate) struct WorkspaceView {
    status: String,
    busy: bool, // an analyze/export thread is running
    // Consumed by the startup greeting or the restored folder's status.
    /// The status chrome (toasts, histogram, title, clipping); see [`ChromeState`].
    chrome: ChromeState,
    /// R23-3, feedback #5 ("the AI is too timid"): how COMMITTED the grade
    /// should be, 0..1. A SEPARATE axis from `style_strength` — that one asks
    /// how much like the user's own past edits, this one how hard to push — and
    /// keeping them apart is the point of the round. Persisted; stored as the
    /// raw fraction and handed to `GradeStrength::new` at the call site, which
    /// owns the clamp.
    /// The reverse-fit's HONESTY BUDGET, 0..1 — its OWN dial in the Reverse-fit
    /// fold (user decision 2026-09-12: a control belongs to the section whose
    /// function it serves, and a function two sections need gets two controls,
    /// not one shared reading). Through v1.3.1 the fit read the Analysis
    /// Strength two folds up, which nobody looking at the fit's own row could
    /// find. Persisted as the raw fraction; `GradeStrength::new` owns the clamp
    /// at the call site, exactly like `grade_strength`.
    /// R23-2, feedback #6: also SHOW the vision model the nearest past photo,
    /// not only its numbers. Persisted, and OFF by default — it puts a second
    /// image on every call of a paid analysis (the checkbox says so).
    /// R23-4, feedback #13 ("let it think"): DEEP THINKING for this photo's
    /// analysis — the structured working in the proposal, one step more
    /// reasoning effort, and a visual judge allowed to converge over more than
    /// one round. Persisted, OFF by default: it is the most expensive switch on
    /// the panel (the checkbox states the call counts), and every unattended
    /// surface (batch, eval) is structurally unable to turn it on.
    /// Step 14 / S2: also write a LOCAL prose description of each record's
    /// grade. Persisted, OFF by default — it downloads 4.3 GB once and makes
    /// every build slower, which is not a cost an upgrade may take unasked.
    /// The folder the reference library was last built FROM — prefilled into
    /// the picker so a rebuild does not mean finding it again. Persisted.
    /// The RAW folder a build looks in for each finished photo's sidecar
    /// settings (`library::settings`). Persisted; `None` = no settings check.
    /// The reference library's status as last READ (`library::summary`),
    /// cached: it parses a file that can reach tens of MB, so it must never
    /// happen per frame. `None` = not read yet.
    /// The AI section's run/library/tab state; see [`AiState`].
    ai: AiState,
    /// A status read is on a worker thread (keeps the first draw off the disk).
    /// A library BUILD is running. Its own flag, never the global `busy`:
    /// building measures every photo in a folder and takes minutes, and freezing
    /// the whole app for that would be worse than the missing entry point this
    /// closes. Only the build button gates on it.
    /// The running build's `(stage, completed, total)` — typed facts, worded
    /// at draw time (L12#4). The STAGE is named because the build is four
    /// phases over the whole folder: a bare pair would sweep 0..N more than
    /// once with nothing to say which sweep the user was looking at.
    // Delivery format + depth (阶段4) — replaces the old save_jpeg bool.
    /// The Export panel's dials and delivery facts; see [`ExportState`].
    exp: ExportState,
    // --- unsaved-edit protection ---
    // What recipe.json last held: the develop of the card that was active at
    // the last save. `active_baseline` joins it with saved_strip so every card
    // is compared to its own persisted state.
    // Mirror of the open photo's pixels.json (the saved baked-master path, if
    // any) so the per-frame ● indicator can compare pixel identity WITHOUT a
    // disk read per frame. Updated at open, save, analyze-save and clear; it is
    // the pixel half of the last-saved active card's baseline.
    /// Mirror of the photo's persisted `variants.json` (resolved form), the
    /// baseline the background-variant dirty test compares the live strip
    /// against. `None` = nothing persisted (the trivial single-card case).
    /// Updated at open and on every strip persist; comparing background
    /// variants against the ACTIVE canvas's `saved_recipe`/`pixels_on_disk`
    /// instead (the pre-v0.22 rule) made any two-origin strip permanently
    /// "unsaved" — the quit dialog then re-armed forever.
    /// The open/photo-source state; see [`OpenState`].
    open: OpenState,
    /// Master decodes in flight, keyed (photo, origin): repeat clicks into a
    /// still-decoding cold card must coalesce, not stack a fresh full-res
    /// decode thread per click.
    /// Decoded cold-variant masters, most-recent last. Keyed by the master's
    /// OWN identity (path + the edge it was shrunk to + its file stamp), the
    /// same rule as `base_cache` — a rewritten pixel master misses, and a px
    /// preference change re-decodes at the new edge instead of serving the
    /// old one.
    /// The open could not READ the saved develop (damaged file, or busy in
    /// another AutoShade process): the canvas baseline is not that save, so
    /// the next explicit save must version what it overwrites (save_xmp runs
    /// the backup gate while this stands).
    // Paste-to-selected wrote the open photo's store: (path, exact recipe
    // written) so Msg::Pasted can advance the ● baseline on full success.
    // The OPEN photo's fresh START (computed by the open worker, v1.17.0):
    // Lightroom's Adobe Color canvas where this machine can render it, else
    // the camera-matched base-look knots (empty for non-RAW / no preview).
    // Reset re-stamps it so "reset" always means the fresh-open look —
    // including on a photo whose legacy save carries no curve and therefore
    // opened dark.
    // The base part `source_preview` was developed under (v1.17.0,
    // `render::BasePart`): a source-based canvas whose recipe names another
    // is re-developed before it is shown (`ensure_source_part`).
    // The part a source re-develop is in flight for, so a held slider does
    // not queue a second one.
    // This photo's in-camera lens profile (open worker) — the twin of
    // photo_start: Reset re-stamps it, legacy toggles re-enable from it.
    // This photo's as-shot WB anchor (open worker) — the third calibration
    // half: Reset re-stamps it, the Temp slider and eyedropper anchor on it.
    // The start baked into `before_tex` (`EditRecipe::start_only`). The Before
    // pane mirrors the canvas recipe's start (Reset / paste / restore can
    // change it), so a mismatch triggers a cheap rebuild in `update`.
    // The title-bar ✕ was intercepted with unsaved edits: show the in-app
    // save-all / discard / cancel layer until the user decides.
    // Local buffer for the selected mask's name TextEdit:
    // (mask index, name at seed time, edited text). Committed on focus loss so
    // a rename is ONE undo step, not one per key; the seed-time name lets a
    // pending rename commit safely when the selection jumps rows (commit only
    // while masks[i].name still equals the seed — index shifts can't misfire).
    /// The painting tools and mask-edit state; see [`MaskEditState`].
    masks: MaskEditState,
    // Arrow-key navigation: gallery index to scroll into view next frame.
    // --- settings / denoise ---
    /// 「🤖 AI Denoise on export」's OWN strength (the Export fold's dial) and
    /// 「🤖 AI Denoise now」's OWN strength (the Detail fold's dial) — two
    /// timings of one denoiser, two dials, neither reaching the other (user
    /// decision 2026-09-12: a control sits in the fold whose verb reads it).
    /// RAW starts at the luminance-grain default; baked sources have
    /// separate saved choices, starting at SCUNet's unchanged 0.5.
    /// The reverse-fit dials and its dialog state; see [`FitState`].
    fit: FitState,
    // R23-6 D: 反推 runs the visual judge BEFORE the persist and buys bounded
    // guided rounds off it. Paid and opt-in, like `fit_ai_judge`, and a
    // REVISION of the R20 decision that the fit's review is informational
    // only — user decision 2026-08-17 ⑥, recorded at `start_fit`.
    // Step-6: opt-in fidelity retry for reimagine — a second billed
    // generation when the first diverges. Paid and opt-in, like `fit_deep`.
    // R23-6 B: an explicitly chosen reverse-fit reference (any finished
    // rendition of this frame — a Lightroom export, a camera JPEG, another
    // RAW's develop). SESSION state, not a preference: it names one file for
    // one photo, and remembering it across an app restart would silently fit
    // the next photo against the last photo's reference. Cleared on open.
    // 2026-09-30 (user decision): the card the reverse-fit solves FROM, by
    // id — `None` = automatic (`negative_index`). Session state, cleared on
    // open, for `fit_ref`'s reason: an id names a card of one photo.
    // v1.17.0: the question a whole-frame reimagine landing asks
    // (`fit_prompt_layer`) — None = not asking. Session state: it names a
    // card of the open photo.
    // Whether to ask it at all (a pref): the prompt's 「Don't ask again」 turns
    // it off, the reverse-fit fold's checkbox turns it back on.
    // Tab hides both side panels (the LR grammar) for an edge-to-edge canvas.
    // Session-only on purpose: relaunching with an invisible library reads as
    // a broken app, so the state never persists.
    // egui derives Tab's focus-traversal direction from raw input BEFORE
    // update() runs, so consuming the key can't stop the first widget from
    // taking focus this frame — which would also kill every focused-none
    // shortcut. Cleared on the NEXT frame instead.
    /// The window shell (settings form, theme, open windows, probes); see [`ShellState`].
    ui: ShellState,
    // --- library / gallery ---
    /// The gallery's own state (scan, thumbnails, selection); see
    /// [`GalleryState`].
    gal: GalleryState,
    /// Decode attempts that FAILED, per gallery index. Bounds the retry: see
    /// `request_thumb`. Not a cache — it holds no pixels, only a count.
    // Decoded-base LRU (recent last): base pixels + the photo's base-look knots.
    // --- region box-select (local-edit target on the After image) ---
    /// The coverage overlay and the AI region; see [`OverlayState`].
    overlay: OverlayState,
    // What a lone click just cleared, kept for one double-click window: the
    // second release of a Fit↔1:1 double-click restores it — the pair's
    // FIRST release already ran the plain-click clear (M18).
    // --- retouch: mask painting + generative fill + heal ---
    // Presence is independent of the texture upload flag: None means one
    // scan is due after a change; Some is an O(1) answer on every frame.
    /// Whose area the canvas holds (`BrushOwner`); the other tools' areas
    /// wait in `paint_store`, by `BrushOwner::slot` (None = blank).
    // (straighten, distortion, (profile distortion, profile CA), (ca_r, ca_b)) at build.
    // The manual CA pair is part of the KEY, not just of the composed profile
    // it feeds: the two bools say whether CA is on, never how far, so moving
    // ca_r from +5 to +50 compared equal and the overlay kept the old warp
    // (the L13-9 defect, one field over).
    // Union of the brush segments painted since the last texture upload
    // (canvas px, [x0,y0,x1,y1] half-open). With no geometry active, only
    // this sub-rectangle is uploaded (set_partial) — brushing used to clone
    // + re-upload the WHOLE canvas on every pointer move.
    /// A LOCAL-ADJUSTMENT brush session rides the same paint canvas as
    /// fill/heal: `(target, erase)` — target `None` = the strokes become a
    /// NEW Bitmap mask on Apply; `Some(i)` = they edit mask `i`'s raster
    /// (seeded below). While `Some`, `paint_mode` is also true so every
    /// existing paint affordance (dispatch, cursor, Esc, `[`/`]`) works.
    /// The brush session's WEIGHT buffer — greyscale source of truth the
    /// Apply bakes to a raster. The RGBA canvas is only its display: seeding
    /// an existing raster through the canvas' 8-bit alpha and back would
    /// drift soft edges by a rounding step per edit cycle.
    /// The generative tools and the recipe clipboard; see [`GenerativeState`].
    genai: GenerativeState,
    /// What to change on the SELECTED card's develop (2026-10-21): the request
    /// 「AI adjust the selected variant」 sends with the card's render and
    /// recipe. Its own field — the Direction box guides a fresh analysis.
    // --- production niceties ---
    // --- diagnostic view layers (UX batch) ---
    // Cached masks-cleared develop the coverage's range weights are judged
    // on — reused while the global (non-mask) recipe is unchanged, so a
    // mask-slider drag rebuilds only the coverage map, not a second develop.
    // First-frame OS titlebar sync (see update()'s head): viewport commands
    // sent from the creation closure are dropped before the loop runs.
    // --- zoom / pan (per-photo, reset on open) ---
    /// The canvas view state; see [`ViewState`].
    view: ViewState,
    // The zoom that puts one texel on one PHYSICAL pixel — computed where
    // vis_px/disp/ppp exist (canvas after_view, each frame) so the `1` key
    // can act at frame start, before layout. 1.0 until a first frame lands.
    // Where the zoom is HEADING: discrete jumps (Fit/1:1/keys/double-click)
    // set only this and `zoom` glides there over ~120 ms (apply_zoom_glide,
    // one glide_step per frame from update());
    // continuous inputs (scroll) and photo opens write both, staying
    // instant. Pan is never animated — view_uv's per-frame clamp re-solves
    // it against the gliding zoom, which eases it for free.
    // Last frame's controls-panel rect: ↑/↓ walk the library EXCEPT over
    // this panel, where the same keys nudge a hovered slider.
    // --- crop tool ---
    /// The crop tool's state; see [`CropState`].
    crop: CropState,
    // (handle, drag start, crop at start, straighten° at start). Handles:
    // 0-3 corners, 4 move-inside, 5-8 edge midpoints (T/B/L/R), 9 = drag
    // OUTSIDE the box → rotate-straighten (the LR gesture).
    // --- manual local adjustments (masks) ---
    /// Which of the selected mask's COMPONENTS the canvas tools target:
    /// `None` = the base geometry. Reset whenever the mask selection moves.
    /// The combine mode the panel's "add shape" buttons will arm next.
    // --- tone-curve editor ---
    curve_channel: usize,                  // CURVE_CHANNELS index: 0=master 1=R 2=G 3=B
    // The control point being dragged, WITH the curve set it belongs to
    // (R25 P6: the global curves and a selected mask's can both be on screen
    // in one frame — see `CurveTarget`). Active channel is `curve_channel`.
    curve_drag: Option<(CurveTarget, usize)>,
    // --- batch recipe copy / paste ---
    // WHICH photo the clipboard came from: pasting back onto that photo may
    // keep its bitmap masks (the rasters are its own); every other target
    // gets them stripped.
    // --- recipe library (named develops, this file's siblings under <store>/recipes) ---
    /// The named-recipe library panel; see [`RecipeLibraryState`].
    rlib: RecipeLibraryState,
    /// The selected library recipe developed on the open photo (the window's
    /// preview pane); rebuilt when the selection, the photo or the file changes.
    // --- WB eyedropper ---
    /// The canvas tools and AI/generation flight flags; see [`ToolsState`].
    tools: ToolsState,
    // --- colour-range sample (Range Mask) ---
    // --- Point Color eyedropper (v1.5.0) ---
    // --- AI lens blur (2026-10-13) ---
    // --- clone stamp ---
    // --- stacking (v1.5.0 Track S) ---
    // The Stack fold's own two dials. Deliberately NOT in `Prefs`: which merge
    // a set of frames wants is a property of that shoot, not a taste — a
    // bracket and a focus sweep are different jobs, and remembering last
    // week's answer would run the wrong one on the first click.
    // --- cancellable retouch/generative workers ---
    // Cancel bumps `gen_epoch`; a worker's Retouched message carries the epoch
    // it was started under and is discarded on mismatch (the canvas is never
    // mutated by an abandoned task). `gen_cancel` is Some while such a worker
    // is in flight — it arms the status-bar ✕ and, for streaming generative
    // calls, is polled between events to stop the download itself.
    /// An `Analyze` worker is still on the wire, cancelled or not.
    ///
    /// Cancelling an analyze is ABANDON-only — `produce_recipe` takes no
    /// cancel flag, so nothing stops the HTTP call — and ✕ clears `busy`,
    /// whose only other job was to gate `logic::look::plan`. Without this the ✕
    /// became a "start another one" button: each press spawned a fresh
    /// high-detail vision request that ran to its stall deadline and was
    /// BILLED, N deep. The flag is cleared when the worker's message lands,
    /// whatever its epoch.
    /// The one AI look in flight, with the epoch/keep facts the landing needs
    /// (`logic::look`).
    // --- variants (版本/变体): parallel renditions of the open photo ---
    // Original + any AI-generated / reverse-fitted versions. The active one
    // drives the sliders, histogram and canvas; switching is lossless (each
    // remembers its own base + recipe), so an AI develop no longer "reverts"
    // when you touch a slider — you're editing that variant's own base.
    /// The user answered "Discard & quit". The close guard must then stop
    /// re-testing unsaved state: discarding by definition leaves the state
    /// dirty, so without this the guard cancelled the close and re-raised
    /// the dialog every frame and the app could not be quit at all. (The
    /// SAVE path needs no such flag since v0.22 — Save-all persists each
    /// photo's whole strip to variants.json, so the way-out re-check finds
    /// the state genuinely clean.)
    /// The card strip's dialog/one-shot state; see [`CardsState`].
    cards: CardsState,
    // --- export pipeline (gap batch F + D2) ---
    // --- preview resolution (gap batch E) ---
    // --- recipe versions (gap batch G): ./out/<stem>.v<N>.recipe.json ---
    /// Advisory names + provenance for `versions`, keyed by number (R24-2).
    /// Refreshed with the list; a number with no entry is simply unnamed —
    /// `.version-meta.json` is not required to exist.
    /// A version rename in flight: (the photo it belongs to, the number, the
    /// name as seeded, the edit buffer). Keyed by the NUMBER — versions are
    /// never re-numbered and a deleted number is never re-issued, so unlike
    /// the mask buffer's row index this key cannot cross-commit onto some
    /// other row. The photo rides along so a commit at a boundary that has
    /// already re-pointed `src_path` still writes to the right develop.
    /// Versions list filter: show only snapshots taken from the ACTIVE
    /// variant (R24-2). Session state, deliberately not persisted.
    /// A VARIANT rename in flight (R24-3): (the photo, the card's id, the
    /// name as seeded, the edit buffer). Keyed by the card's own opaque id,
    /// not its strip index — a background push or delete renumbers indices
    /// while a box is open, and the version buffer's number key showed the
    /// way. Unlike the version half this commits into memory only: a card's
    /// name reaches disk with the strip record, at the next save.
    /// The strip's ✕ is ARMED, not fired (R24-4): the index whose delete the
    /// next click confirms. A variant delete is immediate and irreversible —
    /// the undo stack is per-variant by contract (`reset_history`) and no
    /// tombstone can bring a card back — so it asks first, the same
    /// arm-then-act shape as `xmp_beside_confirm`.
    /// Normal's own sidebar state (`panels/normal.rs`).
    normal: crate::panels::normal::NormalState,
    /// The last line uv printed during the managed Python environment's
    /// install (`panels/settings.rs`).
    /// What the sidecar interpreter was found to be at startup
    /// (`python_env::inspect`); `None` until the first check.
    /// The Portraits panel's own state (`panels/portrait.rs`).
    portrait: crate::panels::portrait::PortraitState,
}

impl WorkspaceView {
    pub(crate) fn status(&self) -> String {
        self.status.clone()
    }

    pub(crate) fn status_mut(&mut self) -> &mut String {
        &mut self.status
    }

    pub(crate) fn busy(&self) -> bool {
        self.busy
    }

    pub(crate) fn chrome(&self) -> &ChromeState {
        &self.chrome
    }

    pub(crate) fn chrome_mut(&mut self) -> &mut ChromeState {
        &mut self.chrome
    }

    pub(crate) fn ai(&self) -> &AiState {
        &self.ai
    }

    pub(crate) fn ai_mut(&mut self) -> &mut AiState {
        &mut self.ai
    }

    #[cfg(test)]
    pub(crate) fn busy_mut(&mut self) -> &mut bool {
        &mut self.busy
    }

    #[cfg(test)]
    pub(crate) fn curve_channel_mut(&mut self) -> &mut usize {
        &mut self.curve_channel
    }

    pub(crate) fn exp(&self) -> &ExportState {
        &self.exp
    }

    pub(crate) fn exp_mut(&mut self) -> &mut ExportState {
        &mut self.exp
    }

    pub(crate) fn open(&self) -> &OpenState {
        &self.open
    }

    pub(crate) fn open_mut(&mut self) -> &mut OpenState {
        &mut self.open
    }

    pub(crate) fn masks(&self) -> &MaskEditState {
        &self.masks
    }

    pub(crate) fn masks_mut(&mut self) -> &mut MaskEditState {
        &mut self.masks
    }

    pub(crate) fn fit(&self) -> &FitState {
        &self.fit
    }

    pub(crate) fn fit_mut(&mut self) -> &mut FitState {
        &mut self.fit
    }

    pub(crate) fn ui(&self) -> &ShellState {
        &self.ui
    }

    pub(crate) fn ui_mut(&mut self) -> &mut ShellState {
        &mut self.ui
    }

    pub(crate) fn gal(&self) -> &GalleryState {
        &self.gal
    }

    pub(crate) fn gal_mut(&mut self) -> &mut GalleryState {
        &mut self.gal
    }

    pub(crate) fn overlay(&self) -> &OverlayState {
        &self.overlay
    }

    pub(crate) fn overlay_mut(&mut self) -> &mut OverlayState {
        &mut self.overlay
    }

    pub(crate) fn genai(&self) -> &GenerativeState {
        &self.genai
    }

    pub(crate) fn genai_mut(&mut self) -> &mut GenerativeState {
        &mut self.genai
    }

    pub(crate) fn view(&self) -> &ViewState {
        &self.view
    }

    pub(crate) fn view_mut(&mut self) -> &mut ViewState {
        &mut self.view
    }

    pub(crate) fn crop(&self) -> &CropState {
        &self.crop
    }

    pub(crate) fn crop_mut(&mut self) -> &mut CropState {
        &mut self.crop
    }

    pub(crate) fn curve_channel(&self) -> usize {
        self.curve_channel
    }

    pub(crate) fn curve_drag(&self) -> Option<(CurveTarget, usize)> {
        self.curve_drag
    }

    pub(crate) fn rlib(&self) -> &RecipeLibraryState {
        &self.rlib
    }

    pub(crate) fn rlib_mut(&mut self) -> &mut RecipeLibraryState {
        &mut self.rlib
    }

    pub(crate) fn tools(&self) -> &ToolsState {
        &self.tools
    }

    pub(crate) fn tools_mut(&mut self) -> &mut ToolsState {
        &mut self.tools
    }

    pub(crate) fn cards(&self) -> &CardsState {
        &self.cards
    }

    pub(crate) fn cards_mut(&mut self) -> &mut CardsState {
        &mut self.cards
    }

    pub(crate) fn normal(&self) -> &crate::panels::normal::NormalState {
        &self.normal
    }

    pub(crate) fn normal_mut(&mut self) -> &mut crate::panels::normal::NormalState {
        &mut self.normal
    }

    pub(crate) fn portrait(&self) -> &crate::panels::portrait::PortraitState {
        &self.portrait
    }

    pub(crate) fn portrait_mut(&mut self) -> &mut crate::panels::portrait::PortraitState {
        &mut self.portrait
    }


    /// Write the status bar's text (the two doors below are the only writers)
    pub(crate) fn set_status(&mut self, status: String) {
        self.status = status;
    }

    pub(crate) fn set_busy(&mut self, busy: bool) {
        self.busy = busy;
    }

    pub(crate) fn set_curve_channel(&mut self, channel: usize) {
        self.curve_channel = channel;
    }

    pub(crate) fn set_curve_drag(&mut self, drag: Option<(CurveTarget, usize)>) {
        self.curve_drag = drag;
    }

    /// The paint canvas and the overlay texture it uploads into: one borrow
    /// because a partial upload READS the pixels and WRITES the texture.
    pub(crate) fn paint_upload_parts(
        &self,
    ) -> (Option<&image::RgbaImage>, Option<&egui::TextureHandle>, Option<[u32; 4]>) {
        (self.masks.mask_paint.as_ref(), self.overlay.mask_tex.as_ref(), self.overlay.mask_dirty_rect)
    }

    /// The brush path's two owners in one borrow: the mask canvas and its
    /// fields, and the overlay's pending dirty rect, are written together.
    pub(crate) fn brush_parts(&mut self) -> (&mut MaskEditState, &mut OverlayState) {
        (&mut self.masks, &mut self.overlay)
    }
}

impl Default for WorkspaceView {
    fn default() -> Self {
        Self {
            status: "Open a photo, or open a folder to browse your library.".into(),
            busy: Default::default(),
            chrome: Default::default(),
            ai: Default::default(),
            exp: Default::default(),
            open: Default::default(),
            masks: Default::default(),
            fit: Default::default(),
            ui: Default::default(),
            gal: Default::default(),
            overlay: Default::default(),
            genai: Default::default(),
            view: Default::default(),
            crop: Default::default(),
            curve_channel: Default::default(),
            curve_drag: Default::default(),
            rlib: Default::default(),
            tools: Default::default(),
            cards: Default::default(),
            normal: Default::default(),
            portrait: Default::default(),
        }
    }
}

impl WorkspaceView {
    /// Any canvas tool armed? (the once-hand-written OR list)
    pub(crate) fn tool_armed(&self) -> bool {
        self.crop().mode
            || self.masks().paint_mode
            || self.tools().clone_mode
            || self.tools().wb_picking
            || self.tools().range_picking.is_some()
            || self.tools().point_color_picking
            || self.masks().placing_mask.is_some()
}

    /// A modal layer is up — the quit confirm or the fit prompt — and owns
    /// the keyboard and the pointer (D13).
    pub(crate) fn modal_up(&self) -> bool {
        self.ui().confirm_quit || self.fit().fit_prompt.is_some()
}

    /// The jpg the Save group delivers.
    pub(crate) fn normal_export_opts(&self) -> autoshade::render::ExportOpts {
        autoshade::render::ExportOpts { jpeg_quality: self.normal().quality.jpeg_quality(), ..Default::default() }
}

    /// This app's destination setting resolved to a directory (`None` = ask) —
    /// [`export_dest_dir`] with the app's own state. Read by the single export,
    /// the batch render and both on-screen echoes, so all four name ONE place.
    pub(crate) fn export_dest_dir(&self) -> Option<PathBuf> {
        export_dest_dir(self.exp().dest, self.exp().last_dir.as_deref())
}

    /// The delivery options the export UI currently dials in (gap batch F) —
    /// shared by the single export, the one-off "Export to…" path and the
    /// batch render.
    pub(crate) fn export_opts(&self) -> autoshade::render::ExportOpts {
        autoshade::render::ExportOpts {
            long_edge: (self.exp().long_edge > 0).then_some(self.exp().long_edge),
            sharpen: self.exp().sharpen.clamp(0.0, 100.0),
            jpeg_quality: self.exp().quality.round().clamp(1.0, 100.0) as u8,
            eight_bit: self.exp().format.eight_bit(),
            color_space: match self.exp().space {
                1 => autoshade::render::ExportColorSpace::DisplayP3,
                2 => autoshade::render::ExportColorSpace::AdobeRgb,
                _ => autoshade::render::ExportColorSpace::Srgb,
            },
        }
}

    /// The batch's folder when the destination setting asks (`None` =
    /// cancelled). ONE destination setting for batch and single alike
    /// (R22-7): the batch used to hardcode ./out while a single export could
    /// be sent anywhere, so the same library came out in two different
    /// folders. "Ask every time" asks ONCE for a folder — a per-photo dialog
    /// across 500 frames is not a delivery flow.
    ///
    /// The chosen folder is NOT remembered here (R22 M1). A pick the
    /// read-only-library guard refuses every photo in delivers nothing, and
    /// remembering it made `ExportDest::LastUsed` point at that folder from
    /// then on — a destination that can only ever refuse. The memory is
    /// written where the files actually LAND (the `ExportOutcome::Batch` arm,
    /// `ok > 0`), which is the same rule the single-export dialog follows one
    /// level down (`export_to_chosen_path` records only when the render
    /// started).
    pub(crate) fn pick_batch_folder(&self) -> Option<PathBuf> {
        let mut dlg = rfd::FileDialog::new();
        if let Some(d) = self.exp().last_dir.clone().filter(|d| d.is_dir()) {
            dlg = dlg.set_directory(d);
        }
        dlg.pick_folder()
}

    /// Same alpha threshold as export. Scan at most once per unknown change,
    /// then both the adjust note and verb read the memo in O(1) every frame.
    pub(crate) fn has_painted_mask(&self) -> bool {
        if let Some(painted) = self.masks().mask_painted.get() {
            return painted;
        }
        let painted = self.masks().mask_paint.as_ref().is_some_and(|m| {
            #[cfg(test)]
            self.overlay().mask_presence_scans.set(self.overlay().mask_presence_scans.get() + 1);
            m.pixels().any(|p| p[3] > 10)
        });
        self.masks().mask_painted.set(Some(painted));
        painted
}

    /// Whether `owner`'s brush is the armed canvas tool: the Stamp paints
    /// through `clone_mode`, the other three through `paint_mode`, and a
    /// live mask-brush session belongs to none of them.
    pub(crate) fn brush_armed(&self, owner: BrushOwner) -> bool {
        self.masks().mask_brush.is_none()
            && self.masks().paint_owner == owner
            && if owner == BrushOwner::Stamp { self.tools().clone_mode } else { self.masks().paint_mode }
}

    /// The one-click group's line, in the session language.
    pub(crate) fn normal_line_text(&self, lang: Lang) -> String {
        if self.normal().building {
            return match self.normal().build_progress {
                Some((done, total)) => trf(
                    lang,
                    K_NORMAL_READING_YOUR_REFERENCE_PHOTOS,
                    &[("done", &done.to_string()), ("total", &total.to_string())],
                ),
                None => tr(lang, K_LOGIC_READING_YOUR_REFERENCE_PHOTOS).into(),
            };
        }
        match &self.normal().line {
            // A cancelled one-click leaves no worker behind to say so.
            NormalLine::Working if self.tools().analyze_inflight => tr(lang, K_LOGIC_MAKING_THE_LOOK).into(),
            NormalLine::Idle | NormalLine::Working => tr(lang, K_NORMAL_PICK_A_LOOK_OR_MATCH_YOUR_OWN_REFERENCE_PHOTOS).into(),
            NormalLine::Done => tr(lang, K_NORMAL_DONE_CTRL_Z_GOES_BACK).into(),
            NormalLine::BuiltIn => tr(lang, K_NORMAL_NO_AI_KEY_SET_BUILT_IN_LOOK_USED).into(),
            NormalLine::Failed(why) => why.clone(),
        }
}

    /// 🤖 Remove blemishes: the Retouch panel's AI heal, held to the face
    /// mask — only the spots whose centre is on the face are healed.
        /// The frame's one place for "Keep skin natural": run after the panels
    /// drew and the workers landed, so a slider, a paste and an AI landing
    /// all meet it before the develop or the undo commit sees the recipe.
    /// Only while the switch is on for this card (`keep_skin_on`).
        /// Does "Keep skin natural" hold the open card's writes: the switch on
    /// for this card.
        /// Does it hold a write landing on `photo` (a paste): the open card's
    /// switch for the open photo, a stashed photo's card's own, and on for a
    /// photo not opened this session — the write replaces the edit opening
    /// would decide it on.
        /// Undo or redo put back an edit past the skin limits (made before the
    /// switch was on): it is put back as it was, so the switch goes off for
    /// the card — the next frame must not pull it back and record a new step
    /// over the redo history.
        /// The open card's switch as a history step records it (None: no photo).
        /// Undo or redo puts back the switch the step was recorded with. The
    /// values are not touched: the step's recipe already is what the switch
    /// left it as.
        /// The open card's key for the per-card switch.
        /// Is "Keep skin natural" on for the open card? On unless the card was
    /// read from disk past the limits or the user turned it off. No photo:
    /// on.
        /// The user's own flip of the switch. Turning it on is the explicit act
    /// that may bring values back within the limits, and the status line
    /// says how many it did.
        /// The photo was just read from disk (its strip settled): every card is
    /// decided afresh against the edit that was read — off where it already
    /// goes past the limits, on (no entry) elsewhere.
        /// Every card of the open photo whose edit goes past the limits: off.
        /// A saved version was just read onto the open card: that card alone is
    /// decided afresh, the same way.
        /// A card forked from the open one takes over its switch — a fork is an
    /// edit, not an opening.
        /// A depth estimate started from the Background group landed: aim the
    /// focus at the face (the skin colour range's sample point is the faces'
    /// centre) and keep the person sharp — instead of the canvas pick the
    /// Lens Blur section arms.
        /// "Keep the person sharp": the blur protects the subject matte (on), or
    /// nothing (off). `false` when there is no blur, or nothing to protect
    /// with.
        /// The first group's line, in the session language.
    pub(crate) fn portrait_line_text(&self, lang: Lang) -> String {
        match &self.portrait().line {
            PortraitLine::Idle => tr(lang, K_PORTRAIT_FIND_THE_PEOPLE_FIRST_THE_SKIN_EYES_AND_BACKGROU).into(),
            PortraitLine::Finding => tr(lang, K_PORTRAIT_FINDING_THE_PEOPLE_AND_FACES).into(),
            PortraitLine::Found { faces: 0, .. } => tr(lang, K_PORTRAIT_NO_FACE_FOUND_IN_THIS_PHOTO).into(),
            PortraitLine::Found { faces: 1, .. } => tr(lang, K_PORTRAIT_FOUND_1_FACE).into(),
            PortraitLine::Found { faces, .. } => trf(lang, K_PORTRAIT_FOUND_FACES, &[("n", &faces.to_string())]),
            PortraitLine::Failed(why) => why.clone(),
            PortraitLine::Looking if self.tools().analyze_inflight => tr(lang, K_LOGIC_MAKING_THE_PORTRAIT_LOOK).into(),
            PortraitLine::Looking => tr(lang, K_PORTRAIT_FIND_THE_PEOPLE_FIRST_THE_SKIN_EYES_AND_BACKGROU).into(),
            PortraitLine::Looked => tr(lang, K_NORMAL_DONE_CTRL_Z_GOES_BACK).into(),
        }
}

    /// The tip every brush paints with: the Feather and Flow dials beside
    /// Brush size (both shared by every brush, as the radius is).
    pub(crate) fn brush_tip(&self) -> BrushTip {
        BrushTip { feather: self.masks().brush_feather, flow: self.masks().brush_flow }
}
}

impl WorkspaceView {
    /// Whether `owner`'s area has strokes (the export's alpha > 10 predicate),
    /// memoised per area exactly as `has_painted_mask` memoises the canvas'.
    pub(crate) fn area_painted(&self, owner: BrushOwner) -> bool {
        if owner == self.masks().paint_owner && self.masks().mask_brush.is_none() {
            return self.has_painted_mask();
        }
        self.masks().paint_store[owner.slot()].as_ref().is_some_and(|a| {
            if let Some(p) = a.painted.get() {
                return p;
            }
            #[cfg(test)]
            self.overlay().mask_presence_scans.set(self.overlay().mask_presence_scans.get() + 1);
            let p = a.canvas.pixels().any(|px| px[3] > 10);
            a.painted.set(Some(p));
            p
        })
    }

    /// PNG bytes of the EXPORT mask: painted → transparent (regenerate / heal
    /// here), unpainted → opaque. None if nothing is painted — mirrors the web.
    pub(crate) fn export_mask_png(&self) -> Option<Vec<u8>> {
        if !self.has_painted_mask() {
            return None;
        }
        export_png_of(self.masks().mask_paint.as_ref()?)
    }

    /// PNG bytes of `owner`'s EXPORT mask (see `export_mask_png`); None when
    /// nothing is painted there.
    pub(crate) fn export_area_png(&self, owner: BrushOwner) -> Option<Vec<u8>> {
        if owner == self.masks().paint_owner && self.masks().mask_brush.is_none() {
            return self.export_mask_png();
        }
        if !self.area_painted(owner) {
            return None;
        }
        export_png_of(&self.masks().paint_store[owner.slot()].as_ref()?.canvas)
    }
}

impl WorkspaceView {
    pub(crate) fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.chrome_mut().toast(kind, text);
    }

    /// A worker finished successfully: status line + a success toast, unbusy.
    pub(crate) fn done(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.set_status(text.clone());
        self.chrome_mut().toast(ToastKind::Success, text);
        self.set_busy(false);
    }

    /// A worker failed: status line + a lingering error toast, unbusy. A single
    /// status line is too easy to miss for a failed export or API call.
    pub(crate) fn fail(&mut self, what: &str, e: impl std::fmt::Display) {
        // `{e:#}`, not `{e}`: anyhow's alternate Display prints the WHOLE
        // chain ("mask refine failed: open image X: The image format could not
        // be determined" instead of just the outermost line). Every `.context`
        // this app attaches — the file, the stage, the cause — was being
        // dropped exactly where the user needed it, in the toast.
        let text = format!("{what}: {e:#}");
        self.set_status(text.clone());
        self.chrome_mut().toast(ToastKind::Error, text);
        self.set_busy(false);
    }
}

impl WorkspaceView {
    pub(crate) fn show_startup_note(&mut self) {
        if let Some(note) = self.chrome_mut().startup_note.take() {
            self.status_mut().push_str(" — ");
            self.status_mut().push_str(&note);
        }
    }

    /// Decoded-base LRU lookup (most-recent entry kept last). Missing metadata
    /// (mtime `None`) matches itself, so the cache still works where mtime is
    /// unavailable — it just loses the staleness guard there.
    pub(crate) fn cached_base(&mut self, path: &std::path::Path, edge: u32) -> Option<OpenedBase> {
        let mtime = file_stamp(path);
        let pixels = pixel_identity(path);
        crate::logic::lru::get(&mut self.open_mut().base_cache, |(p, e, t, px)| {
            p == path && *e == edge && *t == mtime && *px == pixels
        })
    }

    /// Remember a freshly decoded base. The KEY comes from the worker's
    /// pre-read capture (`opened.5`), never from a stat or `preview_edge`
    /// here: the decode takes seconds, an egui dropdown popup outlives
    /// add_enabled_ui(!busy) (so the px combo CAN change mid-flight), and
    /// filing the pixels under completion-time state handed a mid-open
    /// replacement the previous content's pixels AND knots under its own
    /// key — the curve memo's identity-before-read rule, applied to its
    /// missed twin.
    pub(crate) fn remember_base(&mut self, path: &std::path::Path, opened: &OpenedBase) {
        let (edge, mtime, pixels) = opened.5.clone();
        crate::logic::lru::insert(
            &mut self.open_mut().base_cache,
            (path.to_path_buf(), edge, mtime, pixels),
            opened.clone(),
            |(p, e, _, _)| p == path && *e == edge,
            BASE_CACHE_CAP,
        );
    }

    /// The save window's answer (`None` = cancelled).
    pub(crate) fn pick_save_file(&mut self, name: String) -> Option<PathBuf> {
        #[cfg(test)]
        if let Some(pick) = self.normal_mut().file_pick.take() {
            return pick;
        }
        let mut dlg = rfd::FileDialog::new().add_filter("jpg", &["jpg"]).set_file_name(name);
        if let Some(d) = self.exp().last_dir.clone().filter(|d| d.is_dir()) {
            dlg = dlg.set_directory(d);
        }
        dlg.save_file()
    }

    /// The destination setting's folder, or the folder dialog's answer when
    /// it asks (`None` = cancelled).
    pub(crate) fn apply_all_dest(&mut self) -> Option<PathBuf> {
        if let Some(d) = self.export_dest_dir() {
            return Some(d);
        }
        #[cfg(test)]
        if let Some(pick) = self.normal_mut().folder_pick.take() {
            return pick;
        }
        self.pick_batch_folder()
    }

    /// Arm the cancellable-worker state for a retouch/generative task: a fresh
    /// cancel flag (this shows the status-bar ✕) and the epoch its result must
    /// present to be applied. Streaming generative workers also hand the flag
    /// to the lib so the download itself stops at the next event; local
    /// compute is abandon-only (its late result is discarded by epoch).
    pub(crate) fn arm_cancel(&mut self) -> (u64, Arc<std::sync::atomic::AtomicBool>) {
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.tools_mut().gen_cancel = Some(flag.clone());
        (self.tools().gen_epoch, flag)
    }

    /// Glide the zoom one step toward its target — run BEFORE any layout
    /// reads it, so the canvas, the % readout and the pan clamp all see one
    /// value. Extracted as the testable seam (L16-10): tests used to drive
    /// the pure glide_step while the update-loop call could be deleted with
    /// every test green; this method IS the behaviour now, and update() is
    /// one call.
    pub(crate) fn apply_zoom_glide(&mut self, ctx: &egui::Context) {
        if self.view().zoom != self.view().zoom_target {
            let dt = ctx.input(|i| i.stable_dt);
            self.view_mut().zoom = glide_step(self.view().zoom, self.view().zoom_target, dt);
            ctx.request_repaint(); // keep stepping while in flight
        }
    }

    /// The one notification door for every paint-buffer replacement/write.
    /// Callers may supply a known answer only from the pixels they just wrote:
    /// fresh/cleared = false, an actual red brush stamp = true. Import/erase
    /// leave it unknown. Texture uploads clear mask_dirty, never this memo.
    pub(crate) fn paint_mask_changed(&mut self, painted: Option<bool>) {
        self.masks().mask_painted.set(painted);
        self.overlay_mut().mask_dirty = true;
    }

    /// Stash the live owner's area for a mask-brush session, which takes the
    /// canvas; `end_mask_brush` puts it back through `unstash_owner_area`.
    pub(crate) fn stash_owner_area(&mut self) {
        if let Some(cur) = self.masks_mut().mask_paint.take() {
            let slot = self.masks().paint_owner.slot();
            let painted = self.masks().mask_painted.get();
            self.masks_mut().paint_store[slot] =
                Some(PaintArea { canvas: cur, painted: std::cell::Cell::new(painted) });
        }
    }

    /// The Save group's landing in plain words — `None` when no Save-group
    /// delivery is running, or a batch had failures (their error stands).
    pub(crate) fn normal_delivery_words(&mut self, lang: Lang, outcome: &ExportOutcome) -> Option<String> {
        if !std::mem::take(&mut self.normal_mut().delivering) {
            return None;
        }
        match outcome {
            // The file actually written: a numbered save names its number.
            ExportOutcome::Single { out, .. } => {
                let dir = out.parent().map(folder_name).unwrap_or_default();
                let file = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                Some(trf(lang, K_ACTIONS_SAVED_TO, &[("file", &file), ("folder", &dir)]))
            }
            ExportOutcome::Batch { ok, errs, dest, renamed, .. } if errs.is_empty() => {
                let saved = trf(lang, K_ACTIONS_PHOTOS_SAVED_TO, &[("n", &ok.to_string()), ("folder", &folder_name(dest))]);
                Some(if renamed.is_empty() {
                    saved
                } else {
                    let k = renamed.len().to_string();
                    let numbered = trf(lang, K_ACTIONS_OF_THEM_HAVE_A_NUMBER_ADDED_TO_THE_NAME_SO_NO_EA, &[("k", &k)]);
                    format!("{saved}. {numbered}")
                })
            }
            // Every failure was the refusal to save into the photos' own
            // folder (counted by type in the worker): the plain sentence.
            ExportOutcome::Batch { .. } => None,
        }
    }

    /// Put a plain sentence in place of the status a shared landing just
    /// wrote — and in its toast, when it raised one.
    pub(crate) fn reword_landing(&mut self, plain: String) {
        let old = std::mem::replace(self.status_mut(), plain.clone());
        if let Some(t) = self.chrome_mut().toasts.iter_mut().rev().find(|t| t.text == old.as_str()) {
            t.text = plain;
        }
    }

    /// Set the one brush radius, clamped to its range, and flash the ring.
    pub(crate) fn set_brush(&mut self, radius: f32, now: f64) {
        self.masks_mut().brush = radius.clamp(BRUSH_R_MIN, BRUSH_R_MAX);
        self.masks_mut().brush_resized_at = Some(now);
    }

    /// Re-read the library from disk. Synchronous and cheap: the folder holds
    /// a handful of small JSON files.
    pub(crate) fn refresh_recipes(&mut self) {
        self.rlib_mut().recipes = autoshade::recipe_library::list();
        if let Some(sel) = self.rlib().recipe_sel.clone()
            && !self.rlib().recipes.iter().any(|r| r.name == sel)
        {
            self.rlib_mut().recipe_sel = None;
            self.rlib_mut().recipe_rename = None;
            self.rlib_mut().recipe_delete_armed = None;
        }
    }

    /// Report an outcome on both channels (the status line for the record, a
    /// toast for the failure the user must not miss).
    pub(crate) fn recipe_status(&mut self, ok: bool, text: String) {
        self.set_status(text.clone());
        if !ok {
            self.toast(ToastKind::Error, text);
        }
    }

    /// Render one landing outcome's words: the one place a logic-authored
    /// status line and toast become the view's strings.
    pub(crate) fn show_words(&mut self, lang: Lang, words: logic::Words) -> String {
        let status = crate::i18n::text(lang, &words.status);
        self.set_status(status.clone());
        if let Some((tone, text)) = words.toast {
            let kind = match tone {
                logic::Tone::Error => ToastKind::Error,
                logic::Tone::Success => ToastKind::Success,
            };
            self.toast(kind, crate::i18n::text(lang, &text));
        }
        status
    }

    pub(crate) fn catalogue_mut(&mut self, role: ModelRole) -> &mut ModelCatalogue {
        match role {
            ModelRole::Image => &mut self.ui_mut().settings.image_models,
            ModelRole::Analysis => &mut self.ui_mut().settings.analysis_models,
        }
    }

    /// Keep the frame loop alive while any worker (analyze/export/thumbs/
    /// models/master decodes) runs — at a 100 ms poll, not frame rate:
    /// worker completion only surfaces through the mpsc poll, and a
    /// full-rate repaint burned CPU for the whole life of a stalled 600 s AI
    /// call. Input still repaints immediately; 100 ms only bounds COMPLETION
    /// latency. `master_loads` is in the gate because a cold-variant master
    /// decode is NOT `busy` (16-lane scan L06). Called at the END of
    /// update() — after every panel had its chance to spawn (L12-3): inside
    /// poll_workers (the top of the frame) it judged a frame whose spawns
    /// had not happened yet, and the first batch of workers ran with no
    /// completion pump armed.
    pub(crate) fn pump_repaint_gate(&mut self, ctx: &egui::Context) {
        if self.busy()
            || self.gal().thumb_inflight > 0
            || self.ui().settings.image_models.fetching
            || self.ui().settings.analysis_models.fetching
            || !self.open().master_loads.is_empty()
            // R23-2: the style-library build is NOT busy-gated (it runs for
            // minutes and the app stays usable), so without it here the
            // "building…" state would only repaint when a progress tick
            // happens to arrive — a single slow 60 MP decode would freeze the
            // counter for as long as it takes. Same reason the two model
            // fetches are on this list.
            || self.ai().style_build_inflight
            || self.ai().style_info_loading
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

impl WorkspaceView {
    pub(crate) fn clear_mask(&mut self) {
        if let Some(m) = &mut self.masks_mut().mask_paint {
            for p in m.pixels_mut() {
                *p = image::Rgba([0, 0, 0, 0]);
            }
            self.paint_mask_changed(Some(false));
            // The WHOLE canvas changed: a pending brush sub-rect from an
            // uncommitted stroke would make ensure_mask_tex's partial-upload
            // fast path re-upload only that rectangle, leaving previously
            // painted pixels alive in the GPU texture indefinitely.
            self.overlay_mut().mask_dirty_rect = None;
        }
        // The display canvas and the greyscale weight buffer are a
        // DUAL-WRITE pair (the brush writes both; Apply bakes the GRAY) —
        // clearing only the display broke the session's own "bakes exactly
        // what it shows" contract: Apply after Clear still committed the
        // cleared strokes (L15-8). end_mask_brush clears the pair too.
        if let Some(g) = &mut self.masks_mut().mask_brush_gray {
            for p in g.pixels_mut() {
                *p = image::Luma([0]);
            }
        }
        self.masks_mut().paint_last = None;
    }

    /// The twin of `stash_owner_area`, once the session's canvas is gone: the
    /// owner's area returns unscanned; with nothing stashed the canvas stays
    /// as the caller left it (blank).
    pub(crate) fn unstash_owner_area(&mut self) {
        let slot = self.masks().paint_owner.slot();
        if let Some(a) = self.masks_mut().paint_store[slot].take() {
            let painted = a.painted.get();
            self.masks_mut().mask_paint = Some(a.canvas);
            self.paint_mask_changed(painted);
            self.overlay_mut().mask_dirty_rect = None;
            self.masks_mut().paint_last = None;
        }
    }
}

impl WorkspaceView {
/// Uniform failure disclosure for a foreground persist compound — the
/// single mapper the NoWait develop-lock wrappers report through
/// (arch item b: five hand-rolled copies had already drifted into
/// wording bugs). TYPED: `WouldBlock` really is "another AutoShade
/// process owns this develop right now", so it gets the caller's busy
/// wording with its retry hint; any other error is real I/O and must
/// not wear the busy costume — a full disk used to read as "another
/// process is working on this photo".
pub(crate) fn persist_postponed(
        &mut self,
        lang: Lang,
    e: &std::io::Error,
    busy_key: &'static str,
    args: &[(&str, &str)],
) {
    let es = e.to_string();
    let mut a: Vec<(&str, &str)> = args.to_vec();
    a.push(("err", &es));
    let t = if e.kind() == std::io::ErrorKind::WouldBlock {
        trf(lang, busy_key, &a)
    } else {
        trf(lang, K_ACTIONS_NOT_SAVED_A_DEVELOP_STORE_WRITE_FAILED, &a)
    };
    self.set_status(t.clone());
    self.toast(ToastKind::Error, t);
}
}

impl WorkspaceView {
    /// End a live mask-brush session (the Esc teardown, extracted from
    /// `disarm_tools` so the three former hand-copies stay one owner):
    /// strokes live in the canvas + gray buffer only until 「Apply」 bakes
    /// them into a claimed raster, so dropping both IS the cancel. The
    /// canvas is cleared too — fill/heal would otherwise inherit the
    /// brush-mask strokes as a phantom retouch selection. Returns whether a
    /// session was actually live, so a caller whose teardown is NOT
    /// user-initiated can disclose the discarded strokes.
    pub(crate) fn end_mask_brush(&mut self) -> bool {
        if self.masks_mut().mask_brush.take().is_some() {
            self.masks_mut().mask_brush_gray = None;
            self.masks_mut().paint_mode = false;
            self.clear_mask();
            // The owner's area the session displaced comes back (2026-09-27).
            self.unstash_owner_area();
            true
        } else {
            false
        }
    }
}

impl WorkspaceView {
    /// Disarm every canvas tool — the SINGLE owner of the disarm list. Ten
    /// arming sites used to hand-copy these assignments, and the comments at
    /// two of them record real bugs born from copies drifting; every arming
    /// site now calls this before setting its own flag. The crop mode is the
    /// editor's, so the aggregate takes it as an argument.
    ///
    /// Transient gesture anchors die with the tools: a stale place_start or
    /// crop_drag used to hijack the next drag. clone_src (the sampled source
    /// pin) deliberately survives — samples stay for resuming, like paint.
    pub(crate) fn disarm_tools(&mut self, editor: &mut Editor) {
        editor.set_crop_mode(self, false);
        self.masks_mut().paint_mode = false;
        self.tools_mut().clone_mode = false;
        self.tools_mut().wb_picking = false;
        self.tools_mut().range_picking = None;
        self.tools_mut().point_color_picking = false;
        self.tools_mut().lens_focus_picking = false;
        self.masks_mut().placing_mask = None;
        self.masks_mut().place_start = None;
        self.crop_mut().drag = None;
        self.masks_mut().mask_drag = None;
        self.masks_mut().paint_last = None;
        self.end_mask_brush();
        // A preset change armed for the crop tool must die with it — the
        // flag survived Esc / Done / a hold-B detour and rewrote the box as
        // a surprise on the next crop entry (CX5-4).
        self.crop_mut().aspect_pending = false;
    }
}

impl WorkspaceView {
    /// Selection moved: every INDEX-ARMED tool whose target is no longer
    /// the selected mask dies with it — `remap_mask_indices`' twin (that
    /// one covers list SHAPE changes; this one covers which row is live).
    /// ↻ Redraw / add-component are the silent pair: their arming buttons
    /// live inside the selected-mask block and the canvas hint discards the
    /// PlaceTarget, so an armed Redraw(j) reads exactly like a fresh
    /// gradient and the next drag rewrites mask j while the user is looking
    /// at another row. A `NewMask` placement and the non-mask tools (crop,
    /// WB picker, clone) are deliberately spared — they are not bound to a
    /// selection. Returns whether a BRUSH session died (its strokes are
    /// user paint), for the caller's disclosure.
    pub(crate) fn disarm_selection_bound_tools(&mut self, keep: Option<usize>) -> bool {
        if matches!(
            self.masks().placing_mask,
            Some((_, PlaceTarget::Redraw(j) | PlaceTarget::Component(j, _))) if Some(j) != keep
        ) {
            self.masks_mut().placing_mask = None;
            self.masks_mut().place_start = None;
        }
        // The colour sampler drops unconditionally — its 💧 label lives on
        // the row that was just left (the first fix of this class).
        self.tools_mut().range_picking = None;
        if matches!(self.masks().mask_brush, Some((Some(j), _)) if Some(j) != keep) {
            return self.end_mask_brush();
        }
        false
    }
}

impl WorkspaceView {
    /// Bring `owner`'s area onto the canvas. The owner that was there goes to
    /// the store with its presence memo; the requested one comes back the
    /// same way, or blank at the canvas' size when it has none. A live
    /// mask-brush session ends first (its strokes belong to no tool, and
    /// `end_mask_brush` puts the stashed area back before the swap).
    pub(crate) fn select_brush_owner(&mut self, owner: BrushOwner) {
        self.end_mask_brush();
        if owner == self.masks().paint_owner {
            return;
        }
        let Some(cur) = self.masks_mut().mask_paint.take() else {
            // No canvas yet (no photo open): only the label moves.
            self.masks_mut().paint_owner = owner;
            return;
        };
        let (w, h) = cur.dimensions();
        let slot = self.masks().paint_owner.slot();
        let painted = self.masks().mask_painted.get();
        self.masks_mut().paint_store[slot] =
            Some(PaintArea { canvas: cur, painted: std::cell::Cell::new(painted) });
        let (canvas, painted) = match self.masks_mut().paint_store[owner.slot()].take() {
            Some(a) => (a.canvas, a.painted.get()),
            None => (image::RgbaImage::new(w, h), Some(false)),
        };
        self.masks_mut().mask_paint = Some(canvas);
        self.paint_mask_changed(painted);
        // The whole canvas changed (see clear_mask on the pending sub-rect).
        self.overlay_mut().mask_dirty_rect = None;
        self.masks_mut().paint_last = None;
        self.masks_mut().paint_owner = owner;
    }
}

impl WorkspaceView {
    /// Put the brush away — the arm toggle's second click. A live mask-brush
    /// session is a cancel too (R22-3: an orphaned session's Apply row would
    /// bake stale weights), and `end_mask_brush` restores the owner's area.
    pub(crate) fn disarm_brush(&mut self) {
        self.masks_mut().paint_mode = false;
        self.tools_mut().clone_mode = false;
        self.masks_mut().paint_last = None;
        self.end_mask_brush();
    }
}

impl WorkspaceView {
    /// Wipe `owner`'s area, on the canvas or in the store; the other tools'
    /// areas stay.
    pub(crate) fn clear_area(&mut self, owner: BrushOwner) {
        if owner == self.masks().paint_owner && self.masks().mask_brush.is_none() {
            self.clear_mask();
        } else {
            self.masks_mut().paint_store[owner.slot()] = None;
        }
    }
}

impl WorkspaceView {
    /// Re-point every stored mask index (selection, colour-range sampler,
    /// redraw target) after the mask list changed shape — delete, drag-drop
    /// reorder, ⬆/⬇ swap. One place, so an index-carrying tool cannot be
    /// forgotten: a dangling index used to make the range sampler write into
    /// whatever mask slid under it.
    pub(crate) fn remap_mask_indices(&mut self, f: impl Fn(usize) -> Option<usize>) {
        let sel_before = self.masks().sel_mask;
        self.masks_mut().sel_mask = self.masks().sel_mask.and_then(&f);
        // Component selection is meaningful only while the SAME mask stays
        // selected under the same index-space; a moved/vanished mask drops it
        // (its component list travelled with the mask, but every canvas tool
        // re-derives from sel_mask — a stale pair here was the same class of
        // bug as the dangling range sampler).
        if self.masks().sel_mask != sel_before || self.masks().sel_mask.is_none() {
            self.masks_mut().sel_component = None;
        }
        self.tools_mut().range_picking = self.tools().range_picking.and_then(&f);
        self.masks_mut().placing_mask = self.masks_mut().placing_mask.take().and_then(|(k, t)| match t {
            PlaceTarget::NewMask => Some((k, PlaceTarget::NewMask)),
            // A vanished redraw target degrades to a NEW mask placement (the
            // pre-v0.22 `Option<usize>` behaviour, kept); a vanished
            // component target must NOT — appending a component to nothing
            // has no sensible fallback, so the arm disarms.
            PlaceTarget::Redraw(i) => {
                Some((k, f(i).map_or(PlaceTarget::NewMask, PlaceTarget::Redraw)))
            }
            PlaceTarget::Component(i, mode) => {
                f(i).map(|j| (k, PlaceTarget::Component(j, mode)))
            }
        });
        // The pending-rename buffer is index-carrying too: after a delete /
        // reorder its seed-guard (name-at-seed must still match) correctly
        // refused to cross-commit — but the SAME mask at its new index then
        // failed the match as well, and the typed rename silently died.
        self.masks_mut().mask_name_buf =
            self.masks_mut().mask_name_buf.take().and_then(|(j, o, b)| Some((f(j)?, o, b)));
        // The mask-brush session carries its TARGET index: 「Apply」 bakes the
        // stroke into `mask_brush.0`, so after a delete/reorder it painted
        // whatever mask slid under the stale slot — the dangling-range-sampler
        // class again, writing pixel weights instead of range bounds. A moved
        // target follows its mask; a vanished one ends the session exactly
        // like Esc (gray buffer AND paint canvas — see disarm_tools: fill/heal
        // would inherit the strokes as a phantom retouch selection). Not a
        // new-mask fallback: that would resurrect a just-deleted mask.
        if let Some((target, erase)) = self.masks().mask_brush {
            match target.map(&f) {
                None => self.masks_mut().mask_brush = Some((None, erase)),
                Some(Some(j)) => self.masks_mut().mask_brush = Some((Some(j), erase)),
                Some(None) => {
                    self.end_mask_brush();
                }
            }
        }
    }
}

impl WorkspaceView {
    /// The base plate under the canvas was REPLACED (a preview-resolution
    /// re-decode, a cold master landing, a retouch bake, a fresh open): the
    /// paint canvas is resized to it — and any live mask-brush session dies
    /// with the old plate, DISCLOSED. The session's weight buffer is
    /// dimension-locked to the plate it was armed on (`start_mask_brush`
    /// sizes both together), so a stroke stamped after the swap lands at
    /// the wrong coordinates in it — and the always-visible 「✓ Apply」 row
    /// would bake those misplaced weights into a durably-adopted raster. A
    /// variant switch already gets the teardown via `load_active`'s
    /// `disarm_tools`; the async landings never called it (L06#5). The
    /// toast lives HERE so no future plate-replacement door can forget it;
    /// a second rebind in the same landing finds no session and stays
    /// silent.
    pub(crate) fn rebind_paint_canvas(&mut self, lang: Lang, w: u32, h: u32) {
        if self.end_mask_brush() {
            let t = tr(
                lang,
                K_ACTIONS_THE_MASK_BRUSH_SESSION_ENDED_THE_CANVAS_PIXELS_U,
            );
            self.toast(ToastKind::Error, t.to_string());
        }
        self.masks_mut().mask_paint = Some(image::RgbaImage::new(w, h));
        self.paint_mask_changed(Some(false));
        // The other tools' areas were in the old frame too.
        self.masks_mut().paint_store = Default::default();
        self.overlay_mut().mask_tex = None;
        self.overlay_mut().mask_dirty = false;
        self.masks_mut().paint_last = None;
    }
}

impl WorkspaceView {
    /// Cold-master LRU lookup (most-recent kept last) — same shape and same
    /// staleness guard as `cached_base`.
    pub(crate) fn cached_master(
    &mut self,
    origin: &std::path::Path,
    edge: u32,
    ) -> Option<Arc<image::DynamicImage>> {
    let stamp = file_stamp(origin);
    crate::logic::lru::get(&mut self.open_mut().master_cache, |(p, e, t)| {
        p == origin && *e == edge && *t == stamp
    })
    }

    /// Remember a freshly decoded cold master under its own identity —
    /// `remember_base`'s twin. The stamp is the SPAWN-TIME capture riding
    /// the message (L12-2): the old landing-time stat argued it made a
    /// mid-decode rewrite miss, but it did the opposite — the NEW stamp was
    /// filed with the OLD pixels, so the next probe HIT on pixels the file
    /// never held. Keyed pre-read, a rewrite moves the stamp on and misses.
    pub(crate) fn remember_master(
    &mut self,
    origin: &std::path::Path,
    edge: u32,
    stamp: FileStamp,
    img: Arc<image::DynamicImage>,
    ) {
    crate::logic::lru::insert(
        &mut self.open_mut().master_cache,
        (origin.to_path_buf(), edge, stamp),
        img,
        |(p, e, _)| p == origin && *e == edge,
        MASTER_CACHE_CAP,
    );
    }

    pub(crate) fn on_thumb(&mut self, ctx: &egui::Context, generation: u64, idx: usize, img: anyhow::Result<image::DynamicImage>) {
                // Ignore thumbnails from a previous folder generation (their
                // inflight count was already discarded when the folder changed).
                if generation == self.gal().gallery_gen {
                    self.gal_mut().thumb_inflight = self.gal().thumb_inflight.saturating_sub(1);
                    match img {
                        Ok(im) => {
                            let tex = ctx.load_texture(
                                format!("thumb{idx}"),
                                to_color_image(&im),
                                egui::TextureOptions::LINEAR,
                            );
                            self.gal_mut().thumbs.insert(idx, tex);
                        }
                        Err(_) => {
                            // Count the failure and release the queue slot:
                            // `request_thumb` allows three attempts, so a
                            // transient decode failure (AV lock, a slow
                            // share) still recovers on a later frame while
                            // an undecodable file stops for good instead of
                            // respawning decode threads every frame.
                            *self.gal_mut().thumb_fail.entry(idx).or_insert(0) += 1;
                            self.gal_mut().thumb_requested.remove(&idx);
                        }
                    }
                }
    }

    pub(crate) fn on_models(
    &mut self,
    lang: Lang,
    role: ModelRole,
    generation: u64,
    res: anyhow::Result<Vec<autoshade::openai_models::ModelInfo>>,
    ) {
    // Single-flight per role (`fetch_models` refuses to start a second
    // while one flies), so whatever lands, THE flight for this role is
    // over — stale or not, the flag must clear or the fetch button and
    // the auto-probe stay disarmed for the rest of the session.
    self.catalogue_mut(role).fetching = false;
    // …but the ids belong to the world the fetch was LAUNCHED in. A key
    // replaced or a URL swapped mid-flight bumped `gen` (via `clear`);
    // installing this completion would resurrect the ids the OLD
    // credential could see and offer them under the new one's name.
    if generation != self.catalogue_mut(role).generation {
        self.ui_mut().settings.status = crate::i18n::text(lang, &logic::models::discarded_words());
        return;
    }
    match res {
        Ok(models) => {
            // The classification and the line are the logic layer's.
            let cat = logic::models::catalogue(&models, role == ModelRole::Image);
            self.ui_mut().settings.status = crate::i18n::text(lang, &logic::models::installed_words(&cat));
            let slot = self.catalogue_mut(role);
            slot.chat = cat.chat;
            slot.image_gen = cat.image;
            slot.efforts = cat.efforts;
            slot.vision = cat.vision;
        }
        Err(e) => {
            self.ui_mut().settings.status =
                crate::i18n::text(lang, &logic::models::failed_words(&e.to_string()));
        }
    }
    }
}
