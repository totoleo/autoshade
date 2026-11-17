//! The variant strip's commands: the cards' load/switch/push/delete, the
//! fork/normalize helpers, the labels and the row action dispatch. Same
//! `AutoShadeApp` impl as `actions.rs`, split by domain.

use crate::*;

impl Commands<'_> {
    /// Make `self.editor.doc().active`'s recipe + base pixels the live working state and
    /// rebuild the before texture. Per-variant transient state (undo history,
    /// local selection, view) restarts — like a soft re-open; what persists is
    /// each variant's recipe + pixels. Shared by switch / push / delete.
    pub(crate) fn load_active(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let active = self.editor.active();
        let Some(v) = self.editor.doc().variants.get(active) else { return };
        let (vkind, vbase, vorigin) = (v.kind, v.base.clone(), v.origin.clone());
        let recipe = v.recipe.clone();
        self.editor.doc_mut().recipe = recipe;
        self.editor.analysis_mut().rationale = self.editor.doc().recipe.rationale.clone();
        self.editor.analysis_mut().rationale_notes.clear(); // variant recipes carry no typed notes
        // A DISK-restored baked variant (cold strip restore) carries its
        // origin but no decoded base. The master is FULL resolution — a
        // 61 MP TIFF takes seconds — and this runs on the UI thread, so the
        // old inline decode froze the window on the first click into a
        // cold-restored card. Decode on a worker instead: until the pixels
        // land the canvas shows the source develop UNDER A DISCLOSURE (a
        // silent stand-in is the wrong image wearing the right card label),
        // and Msg::MasterLoaded installs by (photo, origin) identity, so a
        // delete/reorder mid-decode discards the late pixels. A re-click
        // while decoding is a no-op — `master_loads` holds the in-flight
        // (photo, origin) pair until the landing clears it — and a re-click
        // after it lands hits `master_cache`: no second decode either way.
        if vbase.is_none()
            && let Some(o) = &vorigin
            && let Some(photo) = self.editor.photo().src_path.clone()
        {
            self.spawn_master_load(ctx, photo, o.clone());
        }
        // The strip's READER joins the repair rule: push_variant and
        // switch_variant sync the OUTGOING canvas into the strip, so a
        // washed Original displaced by an AI push comes back through HERE
        // when its card is clicked — no navigation, no stash, no save
        // involved. Memo-bounded; era-2 recipes short-circuit; a PIXEL-STATE
        // card is skipped like at every other ordering site (its curve is
        // empty by invariant — see `VariantKind::is_source_based`, R24-1).
        // Synchronous first-click cost: the same accepted class as the open
        // gate — one estimate per photo per process when it succeeds, and a
        // transient inability early-exits on the failed probe and retries on
        // the next read.
        if vkind.is_source_based()
            // ...and never while a CROSS-PHOTO open is in flight (src_path
            // already points at the INCOMING photo — the apply_step /
            // live-canvas override hazard); a same-path keep-flight stays
            // admitted. Unreachable today through this fn's callers — the
            // busy interlocks exclude an in-flight open during switch /
            // push / delete — so this is defence-in-depth, like the
            // apply_step gate beside it (whose own callers became
            // busy-gated later), and says so instead of claiming a live
            // scenario.
            && (!self.workspace.open().open_in_flight || (self.workspace.open().open_same_path && self.workspace.cards().keep_recipe))
            && let Some(p) = self.editor.photo().src_path.clone()
            && autoshade::pipeline::repair_pre_era_base_curve(&p, &mut self.editor.doc_mut().recipe).is_some()
        {
            // The strip entry follows the healed canvas (the Ctrl+S rule),
            // and the user is told BY TOAST: the dominant callers
            // (switch_variant, push_variant) overwrite self.workspace.status() in the
            // same frame, so a status write here died before a single paint
            // — the exact defect the load_version site documented.
            self.editor.sync_active_recipe();
            self.workspace.toast(
                ToastKind::Success,
                tr(
                    lang,
                    K_EXPORT_CAMERA_BASE_LOOK_RE_ESTIMATED_THIS_PHOTO_WAS_SAV,
                )
                .to_string(),
            );
        }
        // Base pixels: the variant's own baked raster, else the shared source
        // neutral (Original / Fitted re-develop the same negative).
        let base = vbase.or_else(|| self.editor.photo().source_preview.clone());
        if let Some(base) = base {
            let (mw, mh) = base.dimensions();
            // Before compares under the canvas recipe's own base calibration
            // (empty for legacy recipes — deliberate; a v0.24.0+ fit recipe
            // CARRIES its calibration, so a Fitted variant's Before now shows
            // the camera look instead of the 0.6–1.4 EV dark neutral). Covers
            // BAKED variants too: an InPlace master is a NEUTRAL develop whose
            // recipe keeps its curve (Before must show the camera look on the
            // healed pixels, not the dark neutral), while a Generated
            // variant's recipe carries an empty curve by construction — the
            // same expression serves both.
            let start = self.editor.doc().recipe.start_only();
            self.editor.set_before(self.workspace, ctx, &base, &start);
            self.editor.photo_mut().base_preview = Some(base);
            // A fresh transparent paint mask sized to THIS base (a generated
            // raster and the source neutral can differ in dimensions).
            self.workspace.masks_mut().mask_paint = Some(image::RgbaImage::new(mw, mh));
            self.workspace.paint_mask_changed(Some(false));
            // Every tool's area is per card, like the canvas itself.
            self.workspace.masks_mut().paint_store = Default::default();
            self.workspace.overlay_mut().mask_tex = None;
            self.workspace.overlay_mut().mask_dirty = false;
            self.workspace.masks_mut().paint_last = None;
        }
        self.editor.reset_history(self.workspace); // you can't undo across variants
        self.workspace.overlay_mut().region = None;
        self.workspace.overlay_mut().region_drag = None;
        self.workspace.masks_mut().sel_mask = None;
        self.workspace.masks_mut().sel_component = None;
        // Same boundary rule as open_path: the rename buffer belongs to the
        // variant it was typed on (M15). The strip's own rename box is keyed
        // by CARD ID, not by what is on screen, so it deliberately survives
        // a switch — but an armed ✕ does not: it named an index in the strip
        // the user has just moved away from (R24-4).
        self.workspace.masks_mut().mask_name_buf = None;
        self.workspace.cards_mut().variant_delete_confirm = None;
        self.workspace.overlay_mut().overlay_ref = None;
        self.workspace.overlay_mut().overlay_stale = true;
        self.workspace.chrome_mut().last_rgb = None; // the retained frame belongs to the OLD variant
        // …and so does the After texture (L11-2): its only writers are the
        // develop/retouch landings, so until the new variant's first frame
        // lands the canvas showed the OLD variant's pixels under the NEW
        // variant's controls — seconds, on a large photo. The honest
        // "Preparing preview…" placeholder takes over.
        self.editor.plates_mut().after_tex = None;
        self.workspace.disarm_tools(self.editor);
        self.workspace.tools_mut().clone_src = None; // unlike a mere disarm, a variant switch drops the sample
        self.workspace.view_mut().zoom = 1.0;
        self.workspace.view_mut().zoom_target = 1.0; // instant — a swap must not glide from the old view
        // The `1` key reads LAST frame's cached 1:1 solve; stale across a
        // switch it jumped the new canvas to the old variant's ratio (L11-5).
        self.workspace.view_mut().one_to_one = 1.0;
        self.workspace.view_mut().pan = egui::vec2(0.5, 0.5);
        self.editor.analysis_mut().verdict = None;
        self.editor.session_mut().dirty = true; // re-develop the newly active variant
}

/// Switch the active variant losslessly (strip click): in-flight slider
/// edits are saved back into the variant you leave, then the target's
/// recipe + pixels become current.
pub(crate) fn switch_variant(&mut self, idx: usize, ctx: &egui::Context) {
    if idx == self.editor.doc().active || idx >= self.editor.doc().variants.len() {
        return;
    }
    if self.workspace.busy() {
        // Refusal must be visible — the card stays clickable and a silent
        // no-op reads as a dead UI.
        let t = tr(self.lang, K_ACTIONS_BUSY_VARIANTS_UNLOCK_WHEN_THE_CURRENT_TASK_FINIS);
        self.workspace.toast(ToastKind::Error, t);
        return;
    }
    // Flush every typed-but-uncommitted name INTO the snapshot below
    // (L11-1) — the same boundary rule open_path and Ctrl+C already
    // apply: commit first, then load_active's M15 clear drops the
    // buffer. Clearing without committing silently lost the rename.
    self.commit_pending_names();
    // Read BEFORE the &mut borrow below. `dirty` = an edit awaiting
    // dispatch; `develop_inflight` = a frame the switch is about to make
    // the acceptance gate reject. Either way no completed frame depicts
    // the recipe snapshotted below.
    let no_frame_landed = self.editor.session().dirty || self.editor.scheduler().inflight;
    self.editor.flush_active_variant(no_frame_landed);
    self.editor.doc_mut().active = idx;
    self.load_active(ctx);
    let lang = self.lang;
    let name = tr(lang, self.editor.doc().variants[self.editor.doc().active].kind.label());
    // A baked variant keeps the resolution it was baked at: switching
    // cannot re-decode a master, so the preference and the canvas can
    // legitimately disagree from here on. Said at the moment the
    // disagreement is created — the combo alone would imply the canvas
    // followed it.
    // (Both arms, like every canvas door — see `set_canvas_status`; this
    // one names the variant, so it writes its own pair.)
    *self.workspace.status_mut() = match self.editor.baked_canvas_edge(self.workspace) {
        None => trf(
            lang,
            K_ACTIONS_SWITCHED_TO_VARIANT_VARIANTS_ARE_INDEPENDENT_SWI,
            &[("name", name)],
        ),
        Some(px) => trf(
            lang,
            K_ACTIONS_SWITCHED_TO_VARIANT_ITS_BAKED_PIXELS_STAY_AT_PX,
            &[("name", name), ("px", &px.to_string())],
        ),
    };
}

/// Append a variant and switch to it (its recipe/pixels become live).
/// Saves the outgoing variant's edits first so nothing is lost.
pub(crate) fn push_variant(&mut self, v: Variant, ctx: &egui::Context) {
    // "Nothing is lost" includes a rename still sitting in its TextEdit:
    // both callers are ASYNC completions (reverse-fit, generative edit)
    // that switch variants while the user may be mid-typing, and the
    // switch's M15 boundary clear then discarded the typed name.
    // (switch_variant now flushes the same way — L11-1 — so every
    // variant boundary commits before the M15 clear.)
    self.commit_pending_names();
    // Same guarded card drop as switch_variant (L06#6): the async
    // completion switches away from a variant whose latest edits may
    // never have landed a frame.
    let no_frame_landed = self.editor.session().dirty || self.editor.scheduler().inflight;
    self.editor.flush_active_variant(no_frame_landed);
    self.editor.doc_mut().variants.push(v);
    self.editor.doc_mut().active = self.editor.doc().variants.len() - 1;
    self.load_active(ctx);
}

/// Remove variant `idx` (never the last one — the strip stays non-empty).
/// Only reloads the working state when the ACTIVE variant's identity moves,
/// so deleting a background variant can't clobber live edits. Refuses while
/// busy: an in-flight retouch worker resolves its target by `self.editor.doc().active` at
/// COMPLETION time, so re-anchoring `active` mid-flight would bake its result
/// onto the wrong variant.
pub(crate) fn delete_variant(&mut self, idx: usize, ctx: &egui::Context) {
    if self.workspace.busy() {
        let t = tr(self.lang, K_ACTIONS_BUSY_VARIANTS_UNLOCK_WHEN_THE_CURRENT_TASK_FINIS);
        self.workspace.toast(ToastKind::Error, t);
        return;
    }
    if self.editor.doc().variants.len() <= 1 || idx >= self.editor.doc().variants.len() {
        return;
    }
    // ARM, then act (R24-4). Deleting a card is IRREVERSIBLE in a way
    // deleting a VERSION is not the mirror of, and the asymmetry was
    // silent: undo history is per-variant by contract (`reset_history`
    // clears it at every strip move, and `UndoStep` holds one recipe +
    // one pixel identity, never the card list), no registry can bring a
    // card back the way `.deleted-versions.json` keeps a deleted version
    // deleted, and the strip is the only home a background variant's
    // recipe has until the next save. So the ✕ asks: the first click
    // arms this index (the button relabels itself and the status says
    // what the second click will do), the second deletes. The arm dies
    // with any other strip move — `load_active` clears it.
    if self.workspace.cards().variant_delete_confirm != Some(idx) {
        self.workspace.cards_mut().variant_delete_confirm = Some(idx);
        let lang = self.lang;
        let name = self.variant_label(idx);
        let t = trf(
            lang,
            K_ACTIONS_DELETE_VARIANT_CLICK_AGAIN_TO_CONFIRM_A_DELETED,
            &[("name", &name)],
        );
        self.workspace.set_status(t.clone());
        self.workspace.toast(ToastKind::Error, t);
        return;
    }
    self.workspace.cards_mut().variant_delete_confirm = None;
    // A rename box aimed at the card being deleted dies WITH it — the
    // version rows' rule, on the strip's key.
    if self.workspace.cards().variant_name_buf
        .as_ref()
        .is_some_and(|(_, id, ..)| Some(id.as_str()) == self.editor.doc().variants.get(idx).map(|v| v.id.as_str()))
    {
        self.workspace.cards_mut().variant_name_buf = None;
    }
    let active_removed = idx == self.editor.doc().active;
    self.editor.doc_mut().variants.remove(idx);
    if self.editor.doc().active > idx {
        self.editor.doc_mut().active -= 1; // same variant, shifted left
    }
    if self.editor.doc().active >= self.editor.doc().variants.len() {
        self.editor.doc_mut().active = self.editor.doc().variants.len() - 1; // removed the tail
    }
    if active_removed {
        self.load_active(ctx); // the active variant changed identity
        // …onto a canvas the user did not choose: deleting the active
        // variant can land on a background BAKED one, whose raster the
        // preference cannot re-decode. Silent before — and it carried the
        // deleted canvas's own disclosure forward as if it still applied.
        self.editor.set_canvas_status(self.workspace, self.lang, "variant removed");
    }
}

/// The immutability rule's ACTION (2026-09-13; user decision: 「AI 生图
/// 怎么能在上面继续编辑呢？肯定是新开变体啊」): an edit made while the
/// pristine 「✨ AI generated」 card is active does not land on that card.
/// The live recipe — the edit — moves to a new 「✎ Edited AI image」 card
/// on the same raster, inserted right after the ✨ card, which goes back
/// to neutral and stays the pristine image. The canvas is untouched:
/// same pixels, same recipe, same undo history (Ctrl+Z on the new card
/// returns it to neutral; the ✨ card never held the edit), same view and
/// tools — only the card under the canvas changed, so this deliberately
/// does NOT go through `load_active`, which restarts all of that.
///
/// Called every frame between the panels and the develop dispatch (the
/// sliders, the shortcuts, a landing — every path that writes
/// `self.editor.doc().recipe`), and at every persist boundary before the strip is
/// read (Ctrl+S, ＋, the quit-time Save-all, the stash, the Analyze and
/// version-load installs), so no record ever names the ✨ card as the
/// holder of an edit. A neutral recipe on the ✨ card forks nothing: a
/// Reset there is the pristine image again. Returns whether it forked.
pub(crate) fn fork_edited_card(&mut self) -> bool {
    if self.editor.doc().recipe.is_noop() {
        return false;
    }
    self.fork_from_generated()
}

/// The fork itself, for the one caller that edits PIXELS rather than the
/// recipe (the in-place retouch landing — a heal on the ✨ card must bake
/// into a new ✎ card however neutral the recipe is). Forks only while
/// the active card is the pristine ✨ card.
pub(crate) fn fork_from_generated(&mut self) -> bool {
    let Some(cur) = self.editor.doc().variants.get(self.editor.doc().active) else { return false };
    if cur.kind != VariantKind::Generated {
        return false;
    }
    let forked = Variant {
        kind: VariantKind::Edited,
        id: new_variant_id(),
        name: None,
        recipe: self.editor.doc().recipe.clone(),
        base: cur.base.clone(),
        origin: cur.origin.clone(),
        thumb: None,
    };
    if let Some(cur) = self.editor.active_variant_mut() {
        // Pristine again. Its card keeps the last frame that landed,
        // which is the pristine render: the edit that triggered this
        // fork has not been developed yet (the hook runs before the
        // dispatch). A frame of the neutral recipe that lands late
        // writes to the active card — the ✎ — and the develop already
        // due overwrites it.
        cur.recipe = EditRecipe::default();
    }
    let (from, to) = (self.editor.doc().variants[self.editor.doc().active].id.clone(), forked.id.clone());
    self.editor.insert_variant_after_active(forked);
    // The ✎ card is an edit of this one: it keeps its skin switch.
    self.editor.inherit_keep_skin(self.workspace, &from, &to);
    // A ✕ armed on an index past the insertion now names the wrong card.
    self.workspace.cards_mut().variant_delete_confirm = None;
    let lang = self.lang;
    self.workspace.toast(
        ToastKind::Success,
        tr(
            lang,
            K_ACTIONS_AI_GENERATED_STAYS_AS_GENERATED_YOUR_EDIT_CONTIN,
        ),
    );
    true
}

/// The AI-card invariant applied to a strip that arrived from disk
/// (2026-09-13): every 「✨ AI generated」 card is pristine, and every
/// develop over a generated raster is a 「✎ Edited AI image」 card of its
/// own. A record can say otherwise in one way — a ✨ card carrying edits,
/// which is how every build through v1.3.2 stored an edit made on the
/// generated card. Each such card is split: the edits move to a new ✎
/// card right after it (the same raster, its own minted identity), the
/// ✨ card keeps its identity and name and goes neutral, and the active
/// card follows the edits so the canvas shows what the record described.
/// Returns whether anything changed; the caller says so ONCE by toast —
/// the split is unsaved work (the door does not write), and Ctrl+S is
/// what persists it. (An ✎ card without a ✨ sibling is NOT completed:
/// the user may have deleted the pristine card, and a door that
/// resurrected it would undo that at every open.)
pub(crate) fn normalize_ai_cards(&mut self) -> bool {
    let mut changed = false;
    let mut i = 0;
    while i < self.editor.doc().variants.len() {
        let carries_edits = self.editor.doc().variants[i].kind == VariantKind::Generated
            && !self.editor.doc().variants[i].recipe.is_noop();
        if carries_edits {
            let edits = std::mem::take(&mut self.editor.doc_mut().variants[i].recipe);
            let forked = Variant {
                kind: VariantKind::Edited,
                id: new_variant_id(),
                name: None,
                recipe: edits,
                base: self.editor.doc().variants[i].base.clone(),
                origin: self.editor.doc().variants[i].origin.clone(),
                thumb: None,
            };
            self.editor.doc_mut().variants.insert(i + 1, forked);
            if self.editor.doc().active == i {
                self.editor.doc_mut().active = i + 1;
            } else if self.editor.doc().active > i {
                self.editor.doc_mut().active += 1;
            }
            changed = true;
            i += 1; // past the card just inserted
        }
        i += 1;
    }
    changed
}

/// How card `idx` names itself in a sentence: the user's own name when
/// it has one, else its localized kind label. Owned, so callers can hand
/// it to `trf` while `self` is borrowed mutably.
pub(crate) fn variant_label(&self, idx: usize) -> String {
    match self.editor.doc().variants.get(idx) {
        Some(v) => v.name.clone().unwrap_or_else(|| tr(self.lang, v.kind.label()).to_string()),
        None => String::new(),
    }
}



/// The card [`Self::negative_origin`] reads — its rule, spelled once so
/// the reverse-fit fold can NAME the automatic choice (2026-09-30).
/// Test-only view of [`crate::logic::cards::negative_index`].
#[cfg(test)]
pub(crate) fn negative_index(&self) -> Option<usize> {
    crate::logic::cards::negative_index(&self.editor.doc().variants, self.editor.doc().active)
}

/// The negative's pixels on disk: its master when it carries one, else
/// the loaded file — the input a whole-frame reimagine develops from.
pub(crate) fn negative_path(&self) -> Option<PathBuf> {
    crate::logic::cards::negative_path(&self.editor.doc().variants, self.editor.doc().active, self.editor.photo().src_path.as_deref())
}



/// [`Self::fit_source_index`]'s master on disk (`None` = the loaded
/// file's own frame): what the solve's source frame, the ◭ card's origin
/// and the persisted pixel link all read.
/// Test-only view of [`crate::logic::cards::negative_origin`] for the card
/// assertions: the app's own code asks for the pixels through
/// [`Self::negative_path`] / [`Self::fit_negative`].
#[cfg(test)]
pub(crate) fn negative_origin(&self) -> Option<PathBuf> {
    crate::logic::cards::negative_origin(&self.editor.doc().variants, self.editor.doc().active)
}

pub(crate) fn fit_negative(&self) -> Option<PathBuf> {
    crate::logic::cards::fit_negative(&self.editor.doc().variants, self.editor.doc().active, self.workspace.fit().fit_from.as_deref())
}

/// How card `idx` reads in the reverse-fit's rows: its name and its place
/// in the strip, so two cards of one kind stay apart.
pub(crate) fn numbered_card_label(&self, idx: usize) -> String {
    crate::logic::cards::numbered_label(&self.variant_label(idx), idx)
}

/// The reverse-fit's「From:」menu, spelled once for the two places that
/// offer it — the fit fold and the question a generation landing asks
/// ([`Self::fit_prompt_layer`]): the Automatic entry's label, which names
/// the card [`Self::negative_index`] picks, and every card
/// [`Self::is_fit_source`] accepts, as `(id, label)` in strip order.
pub(crate) fn fit_source_menu(&self) -> (String, Vec<(String, String)>) {
    let cards = crate::logic::cards::fit_source_cards(&self.editor.doc().variants)
        .into_iter()
        .map(|i| (self.editor.doc().variants[i].id.clone(), self.numbered_card_label(i)))
        .collect();
    let auto = crate::logic::cards::automatic_fit_card(&self.editor.doc().variants, self.editor.doc().active)
        .map(|i| self.numbered_card_label(i))
        .unwrap_or_default();
    (trf(self.lang, K_ACTIONS_AUTOMATIC, &[("card", &auto)]), cards)
}

/// R24-3 (#7): copy card `idx`'s develop onto the ▣ Original CARD.
///
/// Two different things the UI has to keep apart: this overwrites the
/// Original card's develop PARAMETERS (its baked pixels and its raster
/// origin stay exactly as they were — the card keeps being the same
/// negative), and it is Ctrl+S that afterwards makes that card's develop
/// the photo's saved develop (`recipe.json`). The SOURCE card survives —
/// Lightroom's 「Set Copy as Original」 rule, and the user decision on
/// record.
///
/// Undo is ONE step, borrowed from the strip's own machinery: the switch
/// lands the canvas on the Original first (which syncs the outgoing card
/// and reseeds history with the Original's OWN develop as the head), the
/// overwrite happens on that canvas, and `commit_now` pushes exactly one
/// step — so Ctrl+Z restores the develop the Original had.
///
/// A PIXEL-STATE source is refused: its look lives in its raster, and the
/// canvas recipe over it is stripped of curve, lens and as-shot anchor by
/// construction, so "applying" it would hand the negative an almost-empty
/// develop. Same judgement (and same remedy) as Ctrl+S's XMP refusal and
/// 「＋ Save as version」's.
pub(crate) fn apply_to_original(&mut self, idx: usize, ctx: &egui::Context) {
    let lang = self.lang;
    if self.workspace.busy() {
        let t = tr(lang, K_ACTIONS_BUSY_VARIANTS_UNLOCK_WHEN_THE_CURRENT_TASK_FINIS);
        self.workspace.toast(ToastKind::Error, t);
        return;
    }
    let Some(src) = self.editor.doc().variants.get(idx) else { return };
    if !src.kind.is_source_based() {
        // Two refusals, one axis: a ✨ card has no parameters at all, an
        // ✎ card's are tuned over AI pixels and mean nothing on the
        // negative (the ▣ button's hover says the same).
        let t = if src.kind == VariantKind::Edited {
            tr(
                lang,
                K_DEVELOP_THIS_CARD_S_DEVELOP_IS_TUNED_OVER_AI_GENERATED_P,
            )
        } else {
            tr(
                lang,
                K_DEVELOP_A_GENERATED_VARIANT_S_LOOK_LIVES_IN_ITS_PIXELS_T,
            )
        };
        self.workspace.set_status(t.into());
        self.workspace.toast(ToastKind::Error, t);
        return;
    }
    let Some(dst) = self.editor.original_index() else {
        let t = tr(
            lang,
            K_ACTIONS_THIS_PHOTO_S_STRIP_HOLDS_NO_ORIGINAL_CARD_TO_APP,
        );
        self.workspace.set_status(t.into());
        self.workspace.toast(ToastKind::Error, t);
        return;
    };
    if dst == idx {
        return; // the negative is already itself
    }
    // A persistence-shaped action: every name box the user has open
    // commits first (U10 / the 10+1 boundary rule).
    self.commit_pending_names();
    // The develop to copy is the LIVE canvas when the source card is the
    // active one — its in-flight slider edits are what the user means by
    // "this look" — and the card's stored recipe otherwise. Captured
    // BEFORE the switch, which writes the canvas back into that card.
    let recipe =
        if idx == self.editor.doc().active { self.editor.doc().recipe.clone() } else { self.editor.doc().variants[idx].recipe.clone() };
    let name = self.variant_label(idx);
    self.switch_variant(dst, ctx);
    if self.editor.doc().active != dst {
        return; // the switch declined (it owns its own refusal message)
    }
    if let Some(o) = self.editor.doc_mut().variants.get_mut(dst) {
        o.recipe = recipe.clone();
        // The card's thumbnail renders the develop we just replaced —
        // the cross-card write rule the strip-healing loop in export.rs
        // already follows. The strip's 「…」 placeholder takes over until
        // the next develop lands.
        o.thumb = None;
    }
    self.editor.doc_mut().recipe = recipe;
    self.editor.resync_recipe_display(self.workspace);
    self.editor.session_mut().dirty = true;
    self.editor.commit_now(self.workspace);
    *self.workspace.status_mut() = trf(
        lang,
        K_ACTIONS_COPIED_ONTO_THE_ORIGINAL_CARD_ITS_PIXELS_ARE_UNT,
        &[("name", &name)],
    );
}

/// Commit a pending VARIANT rename into the card it was typed on
/// (R24-3) — the strip half of the `commit_mask_name_buf` rule (U10).
///
/// Keyed by (photo, card id), both captured at seed time: a background
/// push or delete renumbers strip INDICES while a box is open, so an
/// index key would cross-commit onto another card (the version buffer's
/// number key records the same lesson). Memory only — a card's name
/// reaches disk with the strip record at the next save, which is why
/// `open_dirty_variants` counts a renamed card as unsaved work.
pub(crate) fn commit_variant_name_buf(&mut self) {
    let Some((photo, id, seed, buf)) = self.workspace.cards().variant_name_buf.clone() else { return };
    if buf == seed {
        return;
    }
    if self.editor.photo().src_path.as_deref() != Some(photo.as_path()) || id.is_empty() {
        // The card belongs to a photo that is no longer open (its strip
        // went with it), or to no identifiable card at all: drop the
        // buffer rather than paint the name onto whatever sits here now.
        self.workspace.cards_mut().variant_name_buf = None;
        return;
    }
    // Capped HERE, on the way in, to the same byte limit the store applies
    // on the way out: a longer name would be truncated by
    // `variants_record_bytes` and the live card would then differ from its
    // own saved record for good — a photo permanently counted as unsaved
    // work. Popped char by char so the cut lands on a boundary.
    let mut named = buf.trim().to_string();
    while named.len() > autoshade::store::MAX_STORE_NAME {
        named.pop();
    }
    let named = Some(named).filter(|s| !s.is_empty());
    if let Some(v) = self.editor.doc_mut().variants.iter_mut().find(|v| v.id == id) {
        v.name = named;
    }
    // Re-seed rather than clear (the version rule): the box is very
    // likely still on screen, and a SECOND rename in it has to compare
    // against what was just committed, not against the original.
    self.workspace.cards_mut().variant_name_buf = Some((photo, id, buf.clone(), buf));
}

/// Perform one deferred variant action — **the one owner** (R24-5).
///
/// The card actions are drawn by two surfaces now (the bottom strip and
/// the Versions section's edit-state list), and every one of them mutates
/// `self` while the drawing code still borrows it, so both surfaces defer.
/// Routing them through a single function is what keeps "what ＋ does"
/// from being two answers: the strip's own five-way `else if` chain was
/// already the shape this generalises, and copying it to a second caller
/// is exactly how the two would drift.
///
/// The arms are mutually exclusive by construction — [`VariantAction`] is
/// one value — which preserves the chain's guarantee that one frame cannot
/// switch a card AND delete it.
pub(crate) fn dispatch_variant_action(&mut self, act: VariantAction, ctx: &egui::Context) {
    match act {
        VariantAction::Switch(i) => self.switch_variant(i, ctx),
        VariantAction::Delete(i) => self.delete_variant(i, ctx),
        VariantAction::Apply(i) => self.apply_to_original(i, ctx),
        VariantAction::Rename(id) => {
            // Opening a box COMMITS whichever one was open first — the
            // mask panel's cross-commit rule, made structural by keying on
            // the card's id (R24-3).
            self.commit_variant_name_buf();
            if let Some(photo) = self.editor.photo().src_path.clone() {
                let cur = self.editor.doc().variants
                    .iter()
                    .find(|v| v.id == id)
                    .and_then(|v| v.name.clone())
                    .unwrap_or_default();
                self.workspace.cards_mut().variant_name_buf = Some((photo, id, cur.clone(), cur));
            }
        }
        // The buttons are already disabled while busy; the second read is
        // defence in depth against a future caller path, at the cost of
        // one bool. (`save_version` is the only arm that writes a file
        // straight from the live canvas.)
        VariantAction::SaveVersion => {
            if !self.workspace.busy() {
                self.save_version();
            }
        }
    }
}

/// Flush every typed-but-uncommitted NAME box — masks, versions, variant
/// cards (U10; the N+1 boundary rule).
///
/// ONE owner on purpose: ELEVEN production boundaries each hand-copying
/// the growing list is how a box silently stops being flushed the day a
/// third kind of name appears (R24-2 added the second by copying, R24-3
/// the third). Every entry point that persists, navigates, snapshots or
/// quits calls THIS; the "+1" is each box's own lost-focus commit — three
/// of those, one per box, in `panels/develop.rs`. (Counts re-verified at
/// the R24 round-end review; the doc had said ten.)
pub(crate) fn commit_pending_names(&mut self) {
    self.editor.commit_mask_name_buf(self.workspace);
    self.editor.commit_version_name_buf(self.workspace, self.lang);
    self.commit_variant_name_buf();
}
}
