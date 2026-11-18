//! The edit history's frame work: applying a step back onto the canvas, the
//! pixel refresh/rebind a step can need, and the quarter-turn rotate/sync.
//! The stack itself, its commits and the skin switch each step carries are the
//! `Editor`'s own (`editor.rs`); this module is where a step meets the frame.

use crate::*;

impl Editor {
    /// Resync the display to the recipe that is now in the document — the
    /// rationale and its typed notes, the canvas tools, the pending name/curve
    /// buffers and the index-bound selections. A wholesale swap (undo/redo,
    /// Reset, a version load, an AI apply) goes through here.
    pub(crate) fn resync_recipe_display(&mut self, workspace: &mut WorkspaceView) {
        self.analysis_mut().rationale = self.doc().recipe.rationale.clone();
        // Typed notes described the PREVIOUS rationale — a swap without a
        // clear would render a stale zh text over a different develop. The
        // landings that produce fresh notes install them AFTER this call.
        self.analysis_mut().rationale_notes.clear();
        self.analysis_mut().verdict = None;
        // A wholesale recipe swap (undo/redo/Reset/version load/AI apply)
        // disarms EVERY canvas tool — an armed index-carrying tool would fire
        // into whatever now sits at its remembered index, and a live
        // crop/rotate drag would re-apply its stale start angle over the
        // freshly restored recipe on the very next drag frame.
        workspace.disarm_tools(self);
        workspace.masks_mut().mask_name_buf = None; // stale (index, text) must not cross-commit
        // A curve-point drag in flight when the recipe is swapped (Ctrl+Z
        // mid-drag) kept mutating point i of the RESTORED curve on the next
        // drag frame (M17).
        workspace.set_curve_drag(None);
        // A wholesale recipe swap can shrink the mask list — an out-of-range
        // selection must not linger (every consumer bounds-checks, but the
        // panel would silently deselect anyway; do it deterministically).
        // Same-index-different-mask after undo is a known cosmetic residue —
        // mask identity isn't tracked across history steps.
        workspace.masks_mut().sel_mask = workspace.masks().sel_mask.filter(|&i| i < self.doc().recipe.masks.len());
        // The component selection rides on the mask's OWN list, which the
        // swap can also reshape — same deterministic bound.
        workspace.masks_mut().sel_component = workspace.masks().sel_component.filter(|&c| {
            workspace.masks().sel_mask
                .and_then(|i| self.doc().recipe.masks.get(i))
                .is_some_and(|m| c < m.components.len())
        });
    }

    /// Apply a history step: recipe always; the active variant's pixels only
    /// when the step's identity differs (so plain slider undos stay pixel-free
    /// and O(1)). A restored `base: None` reverts a retouched source variant
    /// to the shared source neutral.
    pub(crate) fn apply_step(&mut self, workspace: &mut WorkspaceView, lang: Lang, step: UndoStep, ctx: &Context) {
        let pixels_differ = !step.same_pixels(&self.session().hist.committed);
        let keep_skin = step.keep_skin;
        self.session_mut().hist.committed = step.clone();
        self.doc_mut().recipe = step.recipe;
        // The history is a READER too: a step recorded while the canvas was
        // washed reinstalls the pre-era pair on undo/redo — and the actor
        // that later promoted the canvas (Reset, Analyze, load_version,
        // Ctrl+S) cannot know which stack entries still hold it (the
        // save-time restamp covers only the pair IT replaced). Repairing on
        // the way back keeps every door to the canvas behind one rule.
        // Cost honesty: when the OPEN succeeded, the worker primed the
        // memo and this is a lookup. But washed steps enter history
        // precisely when the open-time estimate was an INABILITY — uncached
        // by design — so the first traversal here IS the retry point and
        // pays one estimate (the accepted class shared with load_active and
        // the open gate); its answer memoizes and the rest of the washed
        // history heals by lookup. Era-2 installs short-circuit before any
        // I/O; the Generated guard matches every other ordering site; and a
        // CROSS-PHOTO in-flight open is refused — src_path already points
        // at the INCOMING photo while this history belongs to the outgoing
        // one (the live-canvas override records the same hazard), so
        // repairing would estimate the WRONG photo onto this canvas. A
        // same-path preview-resolution re-decode (request AND fact: the
        // keep_recipe request verified against the recorded open_same_path)
        // keeps the pair coherent AND its keep arm preserves history —
        // refusing there made a washed install DURABLE, so it is admitted.
        // A same-path FRESH reopen is NOT: its rebuild discards this repair,
        // so admitting it bought a concurrent decode and a false success
        // toast. `committed` follows the heal, or the next commit_now would
        // push the washed head straight back onto the stack.
        //
        // DEFENCE IN DEPTH since the keyboard undo took the buttons' busy
        // gate: `open_in_flight` implies `busy` (open_path early-returns while
        // busy, sets both, and clears both in one handler), and undo/redo are
        // this fn's only callers — so no live caller reaches here mid-flight
        // any more. The gate stays because ungating the keyboard would
        // resurrect every scenario above, and the test drives it directly.
        if (!workspace.open().open_in_flight || (workspace.open().open_same_path && workspace.cards().keep_recipe))
            && self.doc().variants.get(self.doc().active).is_none_or(|v| v.kind.is_source_based())
            && let Some(p) = self.photo().src_path.clone()
            && autoshade::pipeline::repair_pre_era_base_curve(&p, &mut self.doc_mut().recipe).is_some()
        {
            self.session_mut().hist.committed.recipe = self.doc().recipe.clone();
            self.sync_active_recipe();
            workspace.toast(
                ToastKind::Success,
                tr(
                    lang,
                    K_EXPORT_CAMERA_BASE_LOOK_RE_ESTIMATED_THIS_PHOTO_WAS_SAV,
                )
                .to_string(),
            );
        }
        if pixels_differ {
            if let Some(v) = self.active_variant_mut() {
                v.base = step.base;
                v.origin = step.origin;
            }
            self.refresh_active_pixels(workspace, lang, ctx);
            // The quietest door: undoing to a SUPERSEDED master installs a
            // raster the preference cannot re-decode (the resolution switch
            // repoints only the Arc the canvas held), and undo/redo wrote no
            // status at all — so the combo was left to imply the canvas had
            // followed it. BOTH directions: the redo back to a reachable
            // canvas has to retract the claim, or it stands there false.
            self.set_canvas_status(workspace, lang, "restored the canvas pixels");
        }
        self.session_mut().dirty = true;
        self.resync_recipe_display(workspace);
        // The skin switch comes back as the step had it; a step past the
        // skin limits is then put back as it was (Portraits).
        self.restore_skin_switch(workspace, keep_skin);
        self.release_skin_for_history(workspace);
    }

    /// Rebuild the pixel-derived per-variant state after the active variant's
    /// base changed under the same variant (retouch undo/redo): before pane,
    /// paint canvas, overlay reference and the retained frame all described
    /// the old raster. History is deliberately NOT touched here.
    pub(crate) fn refresh_active_pixels(&mut self, workspace: &mut WorkspaceView, lang: Lang, ctx: &Context) {
        let Some(v) = self.doc().variants.get(self.doc().active) else { return };
        let base = v.base.clone().or_else(|| self.photo().source_preview.clone());
        if let Some(base) = base {
            let (mw, mh) = base.dimensions();
            // Same rule as load_active: the canvas recipe's own curve serves
            // every variant (a Generated recipe's curve is empty by
            // construction; an InPlace master is neutral and needs it).
            let start = self.doc().recipe.start_only();
            self.set_before(workspace, ctx, &base, &start);
            self.photo_mut().base_preview = Some(base);
            workspace.rebind_paint_canvas(lang, mw, mh);
        }
        workspace.overlay_mut().overlay_ref = None;
        workspace.overlay_mut().overlay_stale = true;
        workspace.chrome_mut().last_rgb = None;
    }

    /// Turn the open photo by `delta` clockwise quarter turns — ONE undo step,
    /// the whole develop moving together ([`autoshade::pipeline::rotate_recipe`]
    /// owns geometry + raster masks + the turn count).
    ///
    /// The plates are NOT turned here: `sync_base_turns` sees the changed
    /// `recipe.quarter_turns` on the next frame and moves them, which is the
    /// same path an UNDO of this action takes. One mover, so the two
    /// directions cannot drift.
    pub(crate) fn rotate_photo(&mut self, workspace: &mut WorkspaceView, lang: Lang, delta: u8, ctx: &Context) {
        if !self.can_rotate(workspace) {
            return;
        }
        let Some(src) = self.photo().src_path.clone() else { return };
        // The step BEFORE the turn, so one Ctrl+Z puts it back (7.7).
        self.commit_now(workspace);
        let mut next = self.doc().recipe.clone();
        match autoshade::pipeline::rotate_recipe(&mut next, &src, delta) {
            Ok(_) => {
                self.doc_mut().recipe = next;
                self.session_mut().dirty = true;
                self.sync_base_turns(workspace, lang, ctx);
                self.resync_recipe_display(workspace);
                // …and SEAL it, rather than waiting for `commit_if_settled`
                // on the next frame: a click is already a settled gesture, and
                // the one-frame window let a same-frame Ctrl+Z undo the step
                // BEFORE this one (the hazard `commit_now`'s own doc records).
                self.commit_now(workspace);
            }
            Err(e) => {
                // All-or-nothing (see `rotate_recipe`): the recipe is the
                // untouched clone, so nothing moved and nothing needs undoing.
                // The sentence below covers the DISK too since R28 Batch-3 3b —
                // a failed turn used to leave one orphan PNG per attempt in the
                // develop dir while this toast said nothing was changed.
                let t = trf(
                    lang,
                    K_ACTIONS_COULD_NOT_TURN_THIS_PHOTO_NOTHING_WAS_CHANGED,
                    &[("err", &e.to_string())],
                );
                workspace.toast(ToastKind::Error, t);
            }
        }
    }

    /// Bring the base plates into the frame `recipe.quarter_turns` asks for.
    ///
    /// Called once per frame (`app.rs` update) rather than from each of the
    /// eight places that can change the turn — undo, redo, a variant switch, a
    /// version load, a paste, the rotate buttons. `image`'s rotate is
    /// LOSSLESS and the plate is preview-sized, and the common case is a `u8`
    /// comparison that returns immediately.
    ///
    /// Turning the PIXELS rather than teaching the canvas a rotation is
    /// deliberate, and it is the same decision `render_to_image_in` makes:
    /// every screen↔frame mapping downstream (`view_norm_to_orig`, the crop
    /// handles, the paint canvas, the coverage overlay, the retouch region)
    /// is defined against the plate, so one turn here moves all of them and
    /// none of them needs to know.
    pub(crate) fn sync_base_turns(&mut self, workspace: &mut WorkspaceView, lang: Lang, ctx: &Context) {
        let want = self.doc().recipe.quarter_turns % 4;
        let delta = (want + 4 - self.photo().base_turns % 4) % 4;
        if delta == 0 {
            return;
        }
        // A baked master is on disk in its own frame; `can_rotate` refuses to
        // create a delta over one, and this is the same gate seen from the
        // other side. Every master is written in the EXIF frame (the pixel
        // doors refuse a turned photo — `refuse_pixel_work_on_a_turned_photo`),
        // so what this branch protects is the per-variant `v.base` plate,
        // whose frame nothing tracks: turning the previews under it would
        // put the canvas and the cards in two frames.
        if self.doc().variants.iter().any(|v| v.base.is_some() || v.origin.is_some()) {
            self.photo_mut().base_turns = want;
            return;
        }
        let turn = |p: &Arc<image::DynamicImage>| {
            Arc::new(autoshade::render::turn_image((**p).clone(), delta))
        };
        self.photo_mut().source_preview = self.photo().source_preview.as_ref().map(&turn);
        self.photo_mut().base_preview = self.photo().base_preview.as_ref().map(&turn);
        self.photo_mut().base_turns = want;
        // Everything derived from the plate's frame. The Before texture is
        // rebuilt here rather than invalidated: `set_before` is the only
        // writer, and leaving a stale one on screen for a frame is the
        // sideways-preview bug in miniature. The paint canvas is REBOUND
        // (`refresh_active_pixels`'s rule for a replaced plate): its buffer
        // is dimension-locked to the plate and an odd turn transposes the
        // plate, so a canvas left W×H under an H×W plate put every later
        // brush stroke — and the fill/heal mask exported from it — at the
        // wrong coordinates. A live brush session dies with the old frame,
        // disclosed, exactly as at every other replacement door.
        if let Some(b) = self.photo().base_preview.clone() {
            let start = self.doc().recipe.start_only();
            self.set_before(workspace, ctx, &b, &start);
            let (mw, mh) = b.dimensions();
            workspace.rebind_paint_canvas(lang, mw, mh);
        }
        workspace.overlay_mut().overlay_ref = None;
        workspace.overlay_mut().overlay_stale = true;
        workspace.chrome_mut().last_rgb = None;
        workspace.overlay_mut().mask_tex = None;
        workspace.overlay_mut().mask_overlay_tex = None;
        workspace.overlay_mut().overlay_key = None;
        self.session_mut().dirty = true;
    }

    pub(crate) fn undo(&mut self, workspace: &mut WorkspaceView, lang: Lang, ctx: &Context) {
        if let Some(prev) = self.session_mut().hist.undo_stack.pop() {
            let hist = self.history_mut();
            hist.redo_stack.push(hist.committed.clone());
            self.apply_step(workspace, lang, prev, ctx);
        }
    }

    pub(crate) fn redo(&mut self, workspace: &mut WorkspaceView, lang: Lang, ctx: &Context) {
        if let Some(next) = self.session_mut().hist.redo_stack.pop() {
            let hist = self.history_mut();
            hist.undo_stack.push(hist.committed.clone());
            self.apply_step(workspace, lang, next, ctx);
        }
    }
}
