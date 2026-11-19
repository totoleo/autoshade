//! The open photo's edit as ONE aggregate: what a save persists, what a
//! render reads, what an undo step swaps — owned together so a feature has a
//! home that is not `AutoShadeApp`.
//!
//! Its six state objects are PRIVATE. The only ways in are the read accessors
//! (`doc()`, `photo()`, …), the mutation accessors, and the methods that own a
//! multi-step invariant: `active_variant_mut`, `sync_active_recipe`,
//! `flush_active_variant`, `insert_variant_after_active` and `history_mut`.
//! The point of the aggregate is ownership: these state objects die together
//! when another photo opens and are never splintered across clients.

use crate::*;
use crate::panels::normal::DENOISE_FIRST_DOWNLOAD_MB;
use autoshade::recipe::MaskRole;
use crate::actions::PersistedCard;
use crate::actions::ActiveBaseline;

pub(crate) struct Editor {

    /// The open photo and its decoded plates (see [`PhotoPixels`]).
    photo: PhotoPixels,
    /// The canvas textures and their identity (see [`CanvasPlates`]).
    plates: CanvasPlates,
    /// The latest-wins develop scheduler's counters (see [`DevelopScheduler`]).
    scheduler: DevelopScheduler,
    /// The open document — recipe, cards, active index (see [`Document`]).
    doc: Document,
    /// The AI analysis session (see [`AnalysisSession`]).
    analysis: AnalysisSession,
    // WHERE an export lands (R22-7) — the setting that replaced the
    // Export / Download… button pair. See [`ExportDest`].
    // The folder the last export landed in; ExportDest::LastUsed delivers
    // there, and the ask-dialog reopens there.
    // The 「export .xmp beside the photo」 button is armed for a SECOND click:
    // a sidecar already sits beside this photo (Lightroom's own, or an earlier
    // copy) and the next click overwrites it. Session state, cleared on every
    // photo open and after the copy lands — a two-click confirm rather than a
    // modal, matching how the rest of this panel confirms in place.
    // --- undo / redo (a drag is one step, committed on release). Each step
    // carries the recipe AND the active variant's pixel identity (base Arc +
    // origin), so a baked pixel retouch (heal / clone / denoise) is
    // one undoable step instead of a point of no return. Arc clones share the
    // allocation — only a retouch introduces a new raster into history.
    /// The undo history and the ● baseline; see [`HistoryState`].
    /// The per-photo editing session (see [`EditorSession`]).
    session: EditorSession,
}

impl Editor {
    pub(crate) fn photo(&self) -> &PhotoPixels {
        &self.photo
    }

    pub(crate) fn photo_mut(&mut self) -> &mut PhotoPixels {
        &mut self.photo
    }

    pub(crate) fn plates(&self) -> &CanvasPlates {
        &self.plates
    }

    pub(crate) fn plates_mut(&mut self) -> &mut CanvasPlates {
        &mut self.plates
    }

    pub(crate) fn scheduler(&self) -> &DevelopScheduler {
        &self.scheduler
    }

    pub(crate) fn scheduler_mut(&mut self) -> &mut DevelopScheduler {
        &mut self.scheduler
    }

    pub(crate) fn doc(&self) -> &Document {
        &self.doc
    }

    pub(crate) fn doc_mut(&mut self) -> &mut Document {
        &mut self.doc
    }

    pub(crate) fn analysis(&self) -> &AnalysisSession {
        &self.analysis
    }

    pub(crate) fn analysis_mut(&mut self) -> &mut AnalysisSession {
        &mut self.analysis
    }

    pub(crate) fn session(&self) -> &EditorSession {
        &self.session
    }

    pub(crate) fn session_mut(&mut self) -> &mut EditorSession {
        &mut self.session
    }

    /// The variant the document shows.
    pub(crate) fn active(&self) -> usize {
        self.doc.active
    }

    /// The active variant, mutably — the canvas→card sync's one borrow.
    pub(crate) fn active_variant_mut(&mut self) -> Option<&mut Variant> {
        let active = self.doc.active;
        self.doc.variants.get_mut(active)
    }

    /// Copy the live recipe into the ACTIVE variant (the edit→card flush).
    pub(crate) fn sync_active_recipe(&mut self) {
        let recipe = self.doc.recipe.clone();
        if let Some(cur) = self.doc.variants.get_mut(self.doc.active) {
            cur.recipe = recipe;
        }
    }

    /// Sync the active variant and, when no frame landed for the snapshot,
    /// drop its stale thumb (the switch/push card rule, L06#6).
    pub(crate) fn flush_active_variant(&mut self, drop_thumb: bool) {
        let recipe = self.doc.recipe.clone();
        if let Some(cur) = self.doc.variants.get_mut(self.doc.active) {
            cur.recipe = recipe;
            if drop_thumb {
                cur.thumb = None;
            }
        }
    }

    /// Insert a variant after the active one and make it active.
    pub(crate) fn insert_variant_after_active(&mut self, variant: Variant) {
        let at = (self.doc.active + 1).min(self.doc.variants.len());
        self.doc.variants.insert(at, variant);
        self.doc.active = at;
    }

    /// The undo/redo history — one mutable borrow for a multi-stack move.
    pub(crate) fn history_mut(&mut self) -> &mut HistoryState {
        &mut self.session.hist
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            photo: PhotoPixels::default(),
            plates: Default::default(),
            scheduler: Default::default(),
            doc: Default::default(),
            analysis: Default::default(),
            session: Default::default(),
        }
    }
}

impl Editor {
    pub(crate) fn persisted_card_for(&self, live_position: usize) -> Option<PersistedCard<'_>> {
        let live = self.doc().variants.get(live_position)?;
        let rec = match &self.session().hist.saved_strip {
            Some(rec) => rec,
            None => {
                let trivial = self.doc().variants.len() == 1
                    && live_position == 0
                    && crate::model::strip_is_trivial(
                        live.kind,
                        &live.id,
                        live.name.as_deref(),
                        0,
                    );
                return trivial.then_some(PersistedCard {
                    recipe: &self.session().hist.saved_recipe,
                    origin: self.session().hist.pixels_on_disk.as_deref(),
                });
            }
        };

        if let Some(kind) = VariantKind::from_store_str(&rec.active_kind)
            && AutoShadeApp::persisted_identity_matches(
                live,
                live_position,
                kind,
                rec.active_pos,
                rec.active_id.as_deref(),
            )
        {
            return Some(PersistedCard {
                recipe: &self.session().hist.saved_recipe,
                origin: self.session().hist.pixels_on_disk.as_deref(),
            });
        }

        rec.others.iter().enumerate().find_map(|(i, entry)| {
            let position = if i < rec.active_pos { i } else { i + 1 };
            let kind = VariantKind::from_store_str(&entry.kind)?;
            AutoShadeApp::persisted_identity_matches(
                live,
                live_position,
                kind,
                position,
                entry.id.as_deref(),
            )
            .then_some(PersistedCard {
                recipe: &entry.recipe,
                origin: entry.origin.as_deref(),
            })
        })
}

    /// How many persisted-card mismatches hold work that quitting would discard.
    ///
    /// Dirty means: a live card cannot be matched to the union of the saved
    /// active card (`recipe.json` / `pixels.json`) and `variants.json.others`,
    /// or a matched card differs in kind/name/develop, or a persisted card was
    /// deleted. IDs match first; id-less records fall back to kind + position.
    /// Which card is selected is navigation, not an edit. The OPEN photo's
    /// strip only — the per-photo half of
    /// [`Self::inactive_dirty_variants`]; the stash decision in `open_path`
    /// keys on THIS (summing other photos' stashed variants into the gate
    /// chain-stashed every clean photo the user merely visited).
    ///
    /// The pre-v0.22 rule compared each background variant against the
    /// ACTIVE canvas's `saved_recipe`/`pixels_on_disk`. Those describe a
    /// DIFFERENT variant, so any strip whose cards disagree in origin (every
    /// generate→fit flow) counted dirty forever, no save could clear it, and
    /// the quit dialog re-armed until 「Discard」.
    pub(crate) fn open_dirty_variants(&self) -> usize {
        let saved = self.session().hist.saved_strip.as_ref().map(|rec| {
            use crate::logic::strip::SavedCard;
            let mut cards: Vec<SavedCard<'_>> = Vec::with_capacity(rec.others.len() + 1);
            if let Some(kind) = VariantKind::from_store_str(&rec.active_kind) {
                cards.push(SavedCard {
                    kind,
                    position: rec.active_pos,
                    id: rec.active_id.as_deref(),
                    name: rec.active_name.as_deref(),
                    recipe: &self.session().hist.saved_recipe,
                    origin: self.session().hist.pixels_on_disk.as_deref(),
                });
            }
            cards.extend(rec.others.iter().enumerate().filter_map(|(i, entry)| {
                let kind = VariantKind::from_store_str(&entry.kind)?;
                Some(SavedCard {
                    kind,
                    position: if i < rec.active_pos { i } else { i + 1 },
                    id: entry.id.as_deref(),
                    name: entry.name.as_deref(),
                    recipe: &entry.recipe,
                    origin: entry.origin.as_deref(),
                })
            }));
            cards
        });
        crate::logic::strip::open_dirty_count(&self.doc().variants, self.doc().active, saved.as_deref())
}

    /// The active variant, if a photo is open.
    pub(crate) fn active_variant(&self) -> Option<&Variant> {
        self.doc().variants.get(self.doc().active)
}

    /// Toggle the clipping layer WITHOUT a full redevelop: the overlay is a
    /// pure function of the last developed pixels (retained from the last
    /// accepted frame), so the J key / ▲ button / histogram triangles respond
    /// instantly instead of paying a whole develop (100-300 ms at 2560/4096).
    /// Falls back to a redevelop only when no frame is retained yet.
    /// The pixel reference a Range Mask is judged against: this recipe cut
    /// down to the masks BEFORE mask `i`, with every geometry field stripped.
    ///
    /// The PREFIX, not masks-cleared: the engine evaluates a range on the
    /// pixel as it stands when THIS mask runs — masks stack sequentially, so
    /// a later mask's range sees earlier masks' output (`render.rs`
    /// `apply_masks`); a cleared reference judged mask 2's range on pixels
    /// mask 0/1 had already moved (CX5-6). Geometry runs after develop, so
    /// it is no part of the reference, and `develop_preview_framed` has no
    /// geometry stage — left in, the fields only broke the cache key.
    ///
    /// ONE builder for the coverage overlay (`refresh_mask_overlay`) and the
    /// range eyedropper (`handle_range_pick`), which SHARE the one-slot
    /// `overlay_ref` cache compared by recipe equality. The two used to be
    /// hand-copied, and the copy that forgot the manual CA pair missed the
    /// cache on every click under a non-zero Red/cyan or Blue/yellow, paying
    /// a full preview develop on the UI thread for a 5×5 read.
    pub(crate) fn range_reference_recipe(&self, i: usize) -> EditRecipe {
        crate::logic::range::reference_recipe(&self.doc().recipe, i)
}

    /// The geometric mapping context every interaction boundary needs:
    /// original preview pixel dims + the current straighten angle + the lens
    /// geometry (in-camera profile + manual distortion amount).
    pub(crate) fn geom_ctx(&self) -> ((f32, f32), f32, LensArg) {
        let dims = self.photo().base_preview
            .as_ref()
            .map(|b| {
                let (w, h) = b.dimensions();
                (w as f32, h as f32)
            })
            .unwrap_or((1.0, 1.0));
        (
            dims,
            self.doc().recipe.straighten_deg,
            LensArg {
                // COMPOSED (R25 B3): every interaction map downstream of this
                // bundle must see the same profile the pixels went through,
                // manual CA included.
                profile: autoshade::render::geometry_profile(&self.doc().recipe).into_owned(),
                amount: self.doc().recipe.lens_distortion,
            },
        )
}

    /// Can this photo's sidecar be handed to Lightroom at all? RAW only, and
    /// keyed to the ORIGINAL photo (`src_path`), never the active variant: the
    /// projection belongs to the RAW no matter which rendition is on screen.
    ///
    /// Only a camera RAW has the sidecar convention — `store::
    /// lightroom_sidecar` and `pipeline::write_xmp` draw the same line, and a
    /// baked PNG/TIFF's neighbouring `.xmp` is another program's file, which
    /// this must never offer to overwrite.
    ///
    /// Deliberately NOT gated on the projection existing: that would be a disk
    /// stat per frame, and "save the develop first" is a better answer from the
    /// click (the store's own NotFound message says exactly that) than a button
    /// that is greyed out for a reason nothing explains.
    pub(crate) fn can_export_xmp_beside(&self) -> bool {
        self.photo().src_path.as_deref().is_some_and(autoshade::decode::is_raw)
}

    /// The strip index of the card with `id` (an empty id matches nothing).
    pub(crate) fn card_index(&self, id: &str) -> Option<usize> {
        (!id.is_empty()).then(|| self.doc().variants.iter().position(|v| v.id == id)).flatten()
}

    /// Whether the last analysis left anything to read.
    pub(crate) fn has_analysis_output(&self) -> bool {
        self.analysis().verdict.is_some() || !self.analysis().rationale.is_empty()
}

    /// Does the Local Masks header earn its ●? Exactly the rule the mask ROWS
    /// below use — [`mask_active`], the engine's own non-neutrality test plus the
    /// eye — applied over the list. The header used to carry `n_masks > 0`
    /// instead (R22 #16), which lit the dot for a list of muted or parked masks
    /// and said nothing the「Local Masks ({n})」count did not already say.
    pub(crate) fn masks_section_active(&self) -> bool {
        // R33 §G: the field is in this section and it MOVES PIXELS, so the
        // section's dot has to see it. `renderable()` is the engine's own
        // question, asked here for the same reason `mask_active` is — one
        // rule, so the dot and the render cannot disagree.
        self.doc().recipe.colour_field.as_ref().is_some_and(|f| f.renderable())
            || self.doc().recipe.masks.iter().any(mask_active)
}
}

impl Editor {
    /// The ONE owner for the persisted `(recipe, origin)` of the active card.
    /// A card created after the last save has no baseline and is therefore
    /// unsaved. With no strip record, only the trivial lone Original maps to
    /// `recipe.json` / `pixels.json`.
    pub(crate) fn active_baseline(&self) -> Option<ActiveBaseline<'_>> {
        let saved = self.persisted_card_for(self.doc().active)?;
        Some(ActiveBaseline { recipe: saved.recipe, origin: saved.origin })
    }

    /// The live strip as a persistable [`store::VariantsRecord`]: `None` when
    /// the strip is trivial ([`crate::model::strip_is_trivial`] — recipe.json
    /// plus pixels.json already say everything, and a record would be pure
    /// sidecar noise). Since R24-3 a lone Original card that carries a NAME or
    /// a minted identity is no longer trivial: the record is the only home
    /// either of those has.
    pub(crate) fn current_strip_record(&self) -> Option<autoshade::store::VariantsRecord> {
        let ak = self.active_variant().map_or(VariantKind::Original, |v| v.kind);
        if crate::model::strip_is_trivial(
            ak,
            self.active_variant().map_or("", |v| v.id.as_str()),
            self.active_variant().and_then(|v| v.name.as_deref()),
            self.doc().variants.len().saturating_sub(1),
        ) {
            return None;
        }
        Some(autoshade::store::VariantsRecord {
            extra: Default::default(),
            v: 1,
            active_kind: ak.store_str().to_string(),
            active_pos: self.doc().active,
            others: self.doc().variants
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != self.doc().active)
                .map(|(_, v)| autoshade::store::VariantEntry {
                    extra: Default::default(),
                    kind: v.kind.store_str().to_string(),
                    recipe: v.recipe.clone(),
                    origin: v.origin.clone(),
                    // Hop 1 of 6 (R24-2): the live strip's identities + names
                    // to disk. An EMPTY id is written as absent — the record
                    // must not claim an identity the card does not have.
                    id: crate::model::variant_id_field(&v.id),
                    name: v.name.clone(),
                })
                .collect(),
            active_id: self.active_variant().and_then(|v| crate::model::variant_id_field(&v.id)),
            active_name: self.active_variant().and_then(|v| v.name.clone()),
        })
    }

    /// The on-disk PIXEL SOURCE the active variant renders / retouches /
    /// exports FROM. Any variant whose pixels are baked into a ./out raster — a
    /// reimagine or fill (Generated), OR an in-place heal/clone/denoise on ANY variant —
    /// carries that full-resolution artifact in `origin` and renders from it;
    /// a pristine source-based variant (原片 / 反推 with no pixel retouch) has
    /// `origin = None` and renders from `src_path` (the RAW / loaded image)
    /// developed by the recipe. Retouch and export key off THIS — not raw
    /// `src_path` — so what exports matches what's on screen (WYSIWYG), never
    /// the untouched negative underneath a generated / retouched rendition.
    pub(crate) fn active_source_path(&self) -> Option<PathBuf> {
        match self.active_variant() {
            Some(v) => v.origin.clone().or_else(|| self.photo().src_path.clone()),
            None => self.photo().src_path.clone(),
        }
    }

    /// Does the active card develop an AI-generated raster — the pristine
    /// 「✨ AI generated」 card or a 「✎ Edited AI image」 over the same
    /// pixels ([`VariantKind::on_ai_pixels`])? Such a canvas carries no
    /// calibration (the raster already has the camera look, the lens
    /// geometry and a baked white balance), projects to no Lightroom XMP (no
    /// sidecar reproduces AI pixels), and is what the `pixels.json` flag
    /// records. The one place the two kinds part — the ✨ card is immutable,
    /// the ✎ card is where its edits live — asks `== Generated` directly.
    pub(crate) fn active_on_ai_pixels(&self) -> bool {
        self.active_variant().is_some_and(|v| v.kind.on_ai_pixels())
    }

    /// The one door every click-on-the-photo tool shares: a crosshair while
    /// armed, and on a click the point in the ORIGINAL frame (the frame
    /// the develop and every raster live in), mapped out of the transformed
    /// view. `None` until the click. A tool that samples the canvas base reads
    /// it at [`Self::orig_to_base_norm`] of this point.
    pub(crate) fn clicked_orig_norm(&self, ui: &Ui, resp: &Response, xf: ViewXform) -> Option<(f32, f32)> {
        ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        if !resp.clicked() {
            return None;
        }
        let (nx, ny) = xf.to_norm(resp.interact_pointer_pos()?);
        let ((bw, bh), deg, dist) = self.geom_ctx();
        Some(view_norm_to_orig(nx, ny, (bw, bh), deg, &dist))
    }

    /// The focus point as a small ring while the pick is armed, so the
    /// photographer sees where the current focus sits before moving it.
    pub(crate) fn draw_lens_focus(&self, ui: &Ui, xf: ViewXform) {
        let Some(lb) = self.doc().recipe.lens_blur.as_ref() else { return };
        let ((bw, bh), deg, dist) = self.geom_ctx();
        let (vx, vy) = orig_norm_to_view(lb.focus[0], lb.focus[1], (bw, bh), deg, &dist);
        let c = xf.to_screen(vx, vy);
        let p = ui.painter();
        p.circle_stroke(c, 9.0, Stroke::new(3.0, Color32::from_black_alpha(150)));
        p.circle_stroke(c, 9.0, Stroke::new(1.5, Color32::from_white_alpha(235)));
        p.circle_filled(c, 2.0, Color32::WHITE);
    }
}

impl Editor {
    /// The active card's actual pixels choose the path, including generated
    /// masters under a RAW original. An unopened app displays the RAW dials.
    pub(crate) fn denoise_uses_raw(&self) -> bool {
        crate::logic::noise::uses_raw(self.active_source_path().as_deref())
    }

    /// [`export_file_name`](Self::export_file_name) with the container given
    /// (Normal's Save group always delivers a jpg).
    pub(crate) fn export_file_name_as(&self, ext: &str) -> String {
        let src = self.active_source_path();
        let stem = src
            .as_deref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("out")
            .to_string();
        format!("{stem}.developed.{ext}")
    }

    pub(crate) fn active_canvas_dirty(&self) -> bool {
        if self.photo().src_path.is_none() || self.active_variant().is_none() {
            return false;
        }
        self.active_baseline().is_none_or(|saved| {
            dirty_vs(&self.doc().recipe, saved.recipe)
                || !same_master_opt(
                    self.active_variant().and_then(|v| v.origin.as_deref()),
                    saved.origin,
                )
        })
    }

    /// Per-frame status-bar consumer of [`Self::active_baseline`].
    pub(crate) fn unsaved_marker_dirty(&self) -> bool {
        self.active_canvas_dirty()
    }

    /// Window-close consumer of [`Self::active_baseline`].
    pub(crate) fn quit_guard_open_dirty(&self) -> bool {
        self.active_canvas_dirty()
    }

    /// Navigation-stash consumer: active canvas plus this photo's strip work.
    pub(crate) fn nav_stash_gate_dirty(&self) -> bool {
        self.active_canvas_dirty() || self.open_dirty_variants() > 0
    }

    /// Quit-dialog `PendingSave` consumer: the same open-photo state as the
    /// navigation stash, without counting other photos' stash entries.
    pub(crate) fn pending_save_gate_dirty(&self) -> bool {
        self.active_canvas_dirty() || self.open_dirty_variants() > 0
    }
}

impl Editor {
    /// Persist the open photo's strip beside recipe.json/pixels.json and
    /// advance the mirror. Trivial strip ⇒ clear (no noise file). An error
    /// leaves the mirror untouched, so the unsaved protection stays armed.
    /// Test-only since the single-generation commit (L03): production saves
    /// stage the strip INTO [`autoshade::store::commit_develop`] and advance
    /// the mirror on the commit's success — this remains the strip-half
    /// primitive the guard tests drive in isolation.
    #[cfg(test)]
    pub(crate) fn persist_strip(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        match self.current_strip_record() {
            Some(rec) => {
                autoshade::store::write_variants(path, &rec)?;
                self.session_mut().hist.saved_strip = Some(rec);
            }
            None => {
                autoshade::store::clear_variants(path)?;
                self.session_mut().hist.saved_strip = None;
            }
        }
        Ok(())
    }

    /// The After image with its interaction layers — crop tool, mask placement,
    /// paint canvas, box-select — or the SOURCE flashed in the same rect while
    /// `comparing` (B held). Scroll zooms to the cursor; middle-drag or
    /// Space+drag pans; all image-space handlers map through [`ViewXform`].
    /// The canvas image with its two fades: the B-key compare cross-fades
    /// between the source and the edit, and a swap's first frame fades in
    /// over the frame it replaced. `id` is the texture `comparing` selects;
    /// the other one is painted over it while the compare fade runs.
    pub(crate) fn paint_canvas_image(&mut self, ui: &Ui, rect: Rect, uv: Rect, id: TextureId, comparing: bool) {
        let painter = ui.painter_at(rect);
        let ctx = ui.ctx();
        // 1 = showing the source. Driven by `comparing`, so a held B fades
        // in and its release fades back.
        let to_before = ctx.animate_bool_with_time(Id::new("canvas_compare"), comparing, MOTION_FADE_S);
        let other_alpha = if comparing { 1.0 - to_before } else { to_before };
        let swap = self.plates().canvas_fade.as_ref().map(|(old, t)| (old.clone(), ease_out(t.elapsed().as_secs_f32() / MOTION_FADE_S)));
        match &swap {
            // A swap still landing: the outgoing frame under, the new one
            // fading in on top (from the bare canvas when there was none).
            Some((old, k)) if *k < 1.0 => {
                if let Some(old) = old {
                    painter.image(old.id(), rect, uv, Color32::WHITE);
                }
                painter.image(id, rect, uv, Color32::WHITE.gamma_multiply(*k));
                ctx.request_repaint();
            }
            _ => {
                self.plates_mut().canvas_fade = None;
                painter.image(id, rect, uv, Color32::WHITE);
            }
        }
        let other = if comparing { &self.plates().after_tex } else { &self.plates().before_tex };
        if other_alpha > 0.0
            && let Some(t) = other
        {
            painter.image(t.id(), rect, uv, Color32::WHITE.gamma_multiply(other_alpha));
        }
    }
}

impl Editor {
    /// The edge a BAKE renders at: the canvas's own resolution, not the
    /// global preference.
    ///
    /// They part company whenever the canvas is a baked raster the preference
    /// could not re-decode — a background retouched / Generated variant keeps
    /// its own resolution because switching variants cannot re-decode a
    /// master, and an undo can restore a superseded one. Baking at the
    /// preference there installed a NEW-edge raster under an OLD-edge canvas
    /// and the frame jumped resolution mid-retouch: the same disagreement the
    /// refused resolution switch exists to prevent, one door along.
    /// `preview_edge` remains the DECODE edge (`open_path`) — a source-based
    /// canvas is developed from a source decoded AT it, so those two agree by
    /// construction.
    pub(crate) fn canvas_edge(&self, workspace: &WorkspaceView) -> u32 {
        self.photo().base_preview
            .as_ref()
            .map(|b| b.width().max(b.height()))
            .unwrap_or(workspace.genai().preview_edge)
            .clamp(640, 8192)
    }

    /// The resolution a canvas must DISCLOSE, when it holds baked pixels the
    /// preview preference cannot reach. `None` when there is nothing to say.
    ///
    /// FOUR doors install such a canvas: the strip click, an undo to a
    /// SUPERSEDED master (the re-decode repoints only the Arc the canvas
    /// held), the navigation stash restore (which reinstalls the raster the
    /// photo was left with while this open decoded the source at the current
    /// preference), and deleting the active variant (which re-anchors onto a
    /// background one).
    pub(crate) fn baked_canvas_edge(&self, workspace: &WorkspaceView) -> Option<u32> {
        // A canvas has TWO legitimate resolutions, and measuring against
        // either one alone produced a false claim:
        //  · the PREFERENCE — a recorded master is re-decoded with an
        //    UNGUARDED `thumbnail`, which upscales, so a re-decode lands
        //    exactly there whatever the sensor is;
        //  · what the source DELIVERS — the RAW decode is guarded against
        //    upscaling, so a fresh bake on a sensor smaller than the
        //    preference lands there instead, and is perfectly current.
        // Only a THIRD value is stale: a raster baked under an older
        // preference, or a superseded master an undo brought back. Measuring
        // against the preference alone called every fresh sub-preference bake
        // stale; against the delivered edge alone it called every REOPENED
        // sub-preference master stale ("not the preview preference" while the
        // preference was exactly that). It takes both.
        let delivered = self.photo().source_preview
            .as_ref()
            .map(|s| s.width().max(s.height()))
            .unwrap_or(workspace.genai().preview_edge)
            .clamp(640, 8192);
        let canvas = self.canvas_edge(workspace);
        // The baked-raster gate is DEFENCE IN DEPTH, not load-bearing: a
        // source-based canvas IS the source decode (`load_active` falls back
        // to `source_preview`, and every install keeps the pair in lockstep),
        // so `canvas == delivered` already silences it. No test can kill this
        // conjunct; it stays because a future canvas that is neither would
        // otherwise be announced with a word — "bake" — that would not be
        // true of it.
        (canvas != delivered
            && canvas != workspace.genai().preview_edge
            && self.active_variant().is_some_and(|v| v.base.is_some()))
        .then_some(canvas)
    }

    /// The reverse-fit / style-prompt target: a finished rendition of THIS
    /// frame whose look is to be solved back into develop parameters.
    ///
    /// Two entries, in precedence order (R23-6 B, user decision 2026-08-17
    /// ⑤ — "a second develop of the same frame", i.e. the photographer's own
    /// Lightroom export or the camera's JPEG, not only what this app
    /// generated):
    ///
    ///   1. `fit_ref` — a file the user picked on purpose. It wins, because
    ///      an explicit choice must not be shadowed by whichever variant
    ///      happens to be active.
    ///   2. the ./out raster behind the active variant when that variant is
    ///      an AI-generated rendition — the original entry, unchanged.
    ///
    /// The restriction to (2) was never a property of the fit: `fit.rs`'s
    /// own doc has always said "any finished reference of the SAME frame",
    /// and the CLI `match` has always accepted an arbitrary path. It was a
    /// GUI-side `then(...)` that made the generative path the only reachable
    /// one — and, downstream of that, made "the target is not pixel-aligned"
    /// read as an axiom of the method rather than a consequence of the only
    /// target the desktop app could offer.
    pub(crate) fn fit_target(&self, workspace: &WorkspaceView) -> Option<PathBuf> {
        if let Some(p) = &workspace.fit().fit_ref {
            return Some(p.clone());
        }
        let v = self.active_variant()?;
        // Deliberately NOT `on_ai_pixels()` (R24-1): this is a POLICY about
        // which raster the fit DEFAULTS to, not a claim about the card's
        // axis — an ✎ card sits on the same raster as its ✨ card, but its
        // edits are sliders the user already has, not a look to solve for
        // (the AI panel's empty-state line says so), and re-fitting a fit is
        // not what the button means either.
        (v.kind == VariantKind::Generated).then(|| v.origin.clone()).flatten()
    }

    /// The geometry the canvas tools currently target: the selected mask's
    /// selected COMPONENT when one is selected (and still in bounds), else
    /// the selected mask's base geometry.
    pub(crate) fn sel_target_geometry(&self, workspace: &WorkspaceView) -> Option<&MaskGeometry> {
        let i = workspace.masks().sel_mask.filter(|&i| i < self.doc().recipe.masks.len())?;
        let m = &self.doc().recipe.masks[i];
        match workspace.masks().sel_component {
            Some(c) if c < m.components.len() => Some(&m.components[c].geometry),
            _ => Some(&m.mask),
        }
    }

    /// May the photographer turn this photo right now? (Drives both the
    /// toolbar buttons' enabled state and the plate reconciler's own gate, so
    /// the two can never disagree about which photos rotate.)
    ///
    /// The one exclusion is BAKED PIXELS. A retouched/generated master is a
    /// raster on disk in the frame it was baked in, and this build has no way
    /// to record that a file was written before a turn — so turning the canvas
    /// under it would put the master back sideways on the next open, which is
    /// the silent-corruption shape the whole orientation chain exists to
    /// prevent. Rotate FIRST, bake after: the master is then written in the
    /// turned frame and everything agrees. The whole strip counts, not just
    /// the active card — an inactive variant's master is on disk exactly the
    /// same way. (Registered limitation, R27 A6.)
    ///
    /// An ARMED tool is excluded for a smaller reason: brush and retouch
    /// gestures bind screen coordinates to plate pixels mid-drag, and the
    /// plate is about to move under them.
    pub(crate) fn can_rotate(&self, workspace: &WorkspaceView) -> bool {
        self.photo().src_path.is_some()
            && self.photo().base_preview.is_some()
            && !workspace.busy()
            && !workspace.open().open_in_flight
            && !workspace.masks().paint_mode
            && workspace.overlay().region.is_none()
            && self.doc().variants.iter().all(|v| v.base.is_none() && v.origin.is_none())
            // A fill layer is pixels in the frame it was made in, like a
            // baked master (2026-10-01): the live card and every stored one.
            && self.doc().recipe.pixel_layers.is_empty()
            && self.doc().variants.iter().all(|v| v.recipe.pixel_layers.is_empty())
            // …and so is a lens blur's depth map (2026-10-13): estimated in
            // the frame the photo had, its texels would not follow a turn.
            && self.doc().recipe.lens_blur.is_none()
            && self.doc().variants.iter().all(|v| v.recipe.lens_blur.is_none())
    }

    /// Does the AI area carry state worth a ● on its collapsed header?
    ///
    /// Written next to the panel it describes, and enumerating that panel's OWN
    /// field set in reading order: the verdict `ai_analysis` prints and the
    /// Direction it consumes.
    /// R22 #16 added the taste dials on the ground that a non-default dial is
    /// state. On 2026-09-20 they were removed because they are persisted
    /// preferences, so the dot never went out.
    ///
    /// Deliberately NOT in the set: `style_strength`, `grade_strength`, and
    /// `fit_strength` are remembered dial preferences, not this photo's state;
    /// `reimagine_prompt`, `fit_ai_judge`, `zoned_fit`, and (R23-2)
    /// `send_style_ref_image` / `style_src_dir` are persisted preferences of
    /// paid verbs or library bookkeeping. The dot means "this PHOTO's AI
    /// inputs carry state", and a remembered dial or folder says nothing
    /// about this photo.
    pub(crate) fn ai_section_active(&self, workspace: &WorkspaceView) -> bool {
        self.analysis().verdict.is_some() || !workspace.ai().guidance.is_empty()
    }

    /// Is the noise switch on: the card on screen is a ◈ card, or the
    /// switch's own AI denoise is running.
    pub(crate) fn normal_denoise_on(&self, workspace: &WorkspaceView) -> bool {
        self.active_variant().is_some_and(|v| v.kind == VariantKind::Denoised)
            || (workspace.busy()
                && workspace.normal()
                    .denoise_pair
                    .as_ref()
                    .is_some_and(|d| d.denoised.is_none() && self.photo().src_path.as_ref() == Some(&d.photo)))
    }

    /// Does 「🤖 AI Denoise on export」 run for THIS export? Ticked, and the
    /// active card is not a ◈ Denoised card — whose master is already the
    /// denoised negative, so a second pass (SCUNet over a baked master) would
    /// only smooth it further (2026-09-15).
    pub(crate) fn export_denoise_applies(&self, workspace: &WorkspaceView) -> bool {
        workspace.exp().save_denoise
            && !self.active_variant().is_some_and(|v| v.kind == VariantKind::Denoised)
    }

    /// Why the skin and eyes groups are greyed, or `None` when they work.
    pub(crate) fn portrait_face_gate(&self, workspace: &WorkspaceView, lang: Lang) -> Option<&'static str> {
        if self.doc().recipe.masks.iter().any(|m| matches!(m.role, MaskRole::Face | MaskRole::Skin)) {
            return None;
        }
        let searched = workspace.portrait().found.as_ref().is_some_and(|(p, n)| self.photo().src_path.as_ref() == Some(p) && *n == 0);
        Some(if searched { tr(lang, K_PORTRAIT_NO_FACE_FOUND_IN_THIS_PHOTO) } else { tr(lang, K_PORTRAIT_FIND_THE_PEOPLE_AND_FACES_FIRST_THE_FIRST_GROUP) })
    }
}

impl Editor {
    /// `<stem>.developed.{tif|jpg}` — the file name every destination shares.
    /// The stem follows the ACTIVE variant's pixel source, so a Generated
    /// variant exports under its reimagine stem (and its AI pixels), not the
    /// original's.
    pub(crate) fn export_file_name(&self, workspace: &WorkspaceView) -> String {
        self.export_file_name_as(workspace.exp().format.ext())
    }

    /// What the Export button will do this click — resolved BEFORE anything is
    /// written, so "Ask every time" cannot fall through to a silent write.
    pub(crate) fn export_route(&self, workspace: &WorkspaceView) -> ExportRoute {
        match workspace.export_dest_dir() {
            Some(dir) => ExportRoute::Render(dir.join(self.export_file_name(workspace))),
            None => ExportRoute::Ask,
        }
    }

    pub(crate) fn selected_denoise_strength(&self, workspace: &WorkspaceView, on_export: bool) -> f32 {
        crate::logic::noise::selected_strength(
            self.denoise_uses_raw(),
            on_export,
            workspace.exp().denoise_strength,
            workspace.exp().save_denoise_strength,
            workspace.exp().baked_denoise_strength,
            workspace.exp().baked_save_denoise_strength,
        )
    }

    pub(crate) fn inactive_dirty_variants(&self, workspace: &WorkspaceView) -> usize {
        // Photos NAVIGATED AWAY FROM count too. Since batch 47 a photo is
        // stashed when only a BACKGROUND variant is dirty, so the quit dialog
        // lists it. Save-all persists each stashed photo's strip record along
        // with its active canvas (v0.22), so these are savable — the count
        // deliberately stays conservative (every stashed background variant,
        // not only those differing from their photo's on-disk record: that
        // comparison would cost a disk read per stash entry per check).
        let stashed: usize = workspace.open().nav_stash.values().map(|st| st.others.len()).sum();
        self.open_dirty_variants() + stashed
    }

    /// What quitting RIGHT NOW would cost, in the terms `quit.rs` publishes.
    ///
    /// Built from the same predicates as the close guard in `app.rs`, plus the
    /// one thing that guard reaches by MUTATING: a typed but uncommitted mask,
    /// card or version name. The guard may call `commit_pending_names` because
    /// it runs on the way out; a per-frame reading may not write, so it counts
    /// an uncommitted name as unsaved work directly. Leaving that out would be
    /// the U10 loss over again — an otherwise clean recipe whose pending
    /// rename dies with the process.
    ///
    /// The `||` chain is ordered by cost: `inactive_dirty_variants` walks the
    /// strip deep-comparing recipes, so it is asked last.
    pub(crate) fn quit_state(&self, workspace: &WorkspaceView) -> crate::quit::QuitState {
        crate::quit::QuitState::from_app(workspace.busy(), workspace.open().discard_requested, || {
            workspace.masks().mask_name_buf.is_some()
                || workspace.cards().version_name_buf.is_some()
                || workspace.cards().variant_name_buf.is_some()
                || !workspace.open().nav_stash.is_empty()
                || self.quit_guard_open_dirty()
                || self.inactive_dirty_variants(workspace) > 0
        })
    }

    /// Adjust owns every input, its painted area included (`BrushOwner::Adjust`).
    /// Generated and Edited both read AI pixels; the negative never does.
    pub(crate) fn can_adjust(&self, workspace: &WorkspaceView) -> bool {
        !workspace.busy() && self.active_on_ai_pixels()
            && (workspace.area_painted(BrushOwner::Adjust) || !workspace.genai().adjust_prompt.trim().is_empty())
    }

    /// The lines under the noise switch, given whether the denoise helper is
    /// installed (a process-wide probe, so it is passed in).
    pub(crate) fn noise_lines(&self, workspace: &WorkspaceView, lang: Lang, helper: bool) -> Vec<String> {
        if !helper {
            return vec![tr(lang, K_NORMAL_THIS_DOWNLOAD_DOES_NOT_INCLUDE_THE_NOISE_TOOL).into()];
        }
        // The switch is the Retouch panel's denoise: the preview shows it.
        let mut lines = vec![tr(lang, K_NORMAL_IT_TAKES_A_MOMENT_THE_PREVIEW_THEN_SHOWS_THE_RES).to_string()];
        if workspace.normal().denoise_ready != Some(true) && !self.denoise_uses_raw() {
            lines.push(trf(
                lang,
                K_NORMAL_THE_FIRST_TIME_THE_NOISE_MODEL_IS_DOWNLOADED_ABO,
                &[("mb", &DENOISE_FIRST_DOWNLOAD_MB.to_string())],
            ));
        }
        lines
    }

    /// One-line echo of the current delivery settings for the Export hover —
    /// e.g. "JPEG · 2560 px · q95 · sRGB (universal) · → <delivery>" — so the
    /// state stays glanceable now that the settings live in the Export section
    /// instead of a toolbar row. The DESTINATION is the last segment (R22-7):
    /// it is the one delivery setting whose wrong value sends a file somewhere
    /// the user then has to hunt for.
    pub(crate) fn export_summary(&self, workspace: &WorkspaceView, lang: Lang) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.push(tr(lang, workspace.exp().format.label()).to_string());
        parts.push(if workspace.exp().long_edge == 0 {
            tr(lang, K_EXPORT_ORIGINAL_SIZE).to_string()
        } else {
            format!("{} px", workspace.exp().long_edge)
        });
        if workspace.exp().format == ExportFormat::Jpeg {
            parts.push(format!("q{:.0}", workspace.exp().quality));
        }
        if workspace.exp().sharpen > 0.0 {
            parts.push(format!("{} {:.0}", tr(lang, K_EXPORT_OUTPUT_SHARPENING), workspace.exp().sharpen));
        }
        parts.push(tr(lang, EXPORT_SPACES[(workspace.exp().space as usize).min(2)]).to_string());
        if self.export_denoise_applies(workspace) {
            // With its amount: the dial under the checkbox is the one number
            // that decides how much of the minutes-long sidecar result lands.
            // Absent on a ◈ card, where the pass sits out (the echo says what
            // will actually land).
            parts.push(format!(
                "{} {:.0}%",
                tr(lang, K_EXPORT_AI_DENOISE),
                self.selected_denoise_strength(workspace, true) * 100.0
            ));
        }
        parts.push(match workspace.export_dest_dir() {
            // Absolute, like every other place a target is named now: "./out"
            // is relative to whatever directory the app was launched from, and
            // that is exactly the thing users could not find.
            Some(d) => format!("→ {}", abs_display(&d)),
            None => format!("→ {}", tr(lang, ExportDest::Ask.label())),
        });
        parts.join(" · ")
    }
}

impl Editor {
    /// 🎯 armed: a click sets the focus point in the original frame, the one
    /// the develop and the depth map share.
    pub(crate) fn handle_lens_focus_pick(
        &self,
        ui: &Ui,
        resp: &Response,
        xf: ViewXform,
    ) -> Option<logic::Action> {
        let (nx, ny) = self.clicked_orig_norm(ui, resp, xf)?;
        Some(logic::Action::Lens(logic::lens_blur::LensAction::FocusPicked([nx, ny])))
    }
}

impl Editor {
    /// Load the Before texture: `base` with `curve` (a camera-matched base
    /// look) applied. Lightroom's "Before" is the profile-applied default
    /// render, not the linear negative — without the curve, Before sat
    /// 0.6–1.4 EV under After's own starting point and every compare
    /// exaggerated the edit. Baked rasters pass an empty curve (their pixels
    /// already carry the look).
    /// The Before pane: `base` under the canvas recipe's START alone
    /// (`EditRecipe::start_only` — its camera curve, or since v1.17.0 the
    /// creative Look's baked half on its profile's base), every slider at 0.
    pub(crate) fn set_before(&mut self, workspace: &mut WorkspaceView, ctx: &Ctx, base: &image::DynamicImage, start: &EditRecipe) {
        let img = if start.start_only() == EditRecipe::default() {
            to_color_image(base)
        } else {
            to_color_image(&autoshade::render::develop_preview(base, &start.start_only()))
        };
        self.plates_mut().before_tex = Some(ctx.load_texture("before", img, TextureOptions::LINEAR));
        workspace.open_mut().before_start = start.start_only();
    }

    /// The visible full-frame uv window: committed crop (shown cropped, like
    /// Lightroom — except while the crop tool is open, which needs the full
    /// frame) narrowed by zoom/pan. `pan` is stored in crop-window coords and
    /// re-clamped here so edge panning never accumulates out of range.
    pub(crate) fn view_uv(&mut self, workspace: &mut WorkspaceView) -> Rect {
        let win = match (&self.doc().recipe.crop, workspace.crop().mode) {
            (Some(c), false) => Rect::from_min_max(
                pos2(c.left.min(c.right), c.top.min(c.bottom)),
                pos2(c.right.max(c.left), c.bottom.max(c.top)),
            ),
            _ => Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        };
        let half = 0.5 / workspace.view().zoom.clamp(1.0, 12.0);
        workspace.view_mut().pan = vec2(
            workspace.view().pan.x.clamp(half, 1.0 - half),
            workspace.view().pan.y.clamp(half, 1.0 - half),
        );
        Rect::from_min_max(
            pos2(
                win.min.x + (workspace.view().pan.x - half) * win.width(),
                win.min.y + (workspace.view().pan.y - half) * win.height(),
            ),
            pos2(
                win.min.x + (workspace.view().pan.x + half) * win.width(),
                win.min.y + (workspace.view().pan.y + half) * win.height(),
            ),
        )
    }

    pub(crate) fn ensure_mask_tex(&mut self, workspace: &mut WorkspaceView, ctx: &Ctx) {
        // The canvas lives in the ORIGINAL frame (strokes are mapped there so
        // fill/heal edit source pixels), but the texture is blitted over the
        // After image, which has been through distortion + straighten — the
        // overlay must go through the SAME remap or every stroke displays
        // offset under a straightened view. Rebuild when the transform moved,
        // not only when the canvas was painted.
        // The overlay's remap depends on the PROFILE DISTORTION toggle (CA
        // never moves an overlay), so it joins the transform key: switching
        // it must re-blit the canvas (knot data only changes per photo, and
        // opening a photo resets the texture anyway).
        // BOTH profile toggles key the cache (L13-9): collapsed to one
        // bool, "distortion + CA on → turn one off" still compared equal and
        // the overlay kept the OLD warp — strokes floated off their pixels.
        let profile_geom = (
            self.doc().recipe.lens_profile.distortion_on && !self.doc().recipe.lens_profile.distortion.is_empty(),
            self.doc().recipe.lens_profile.ca_on
                && !(self.doc().recipe.lens_profile.ca_r.is_empty()
                    && self.doc().recipe.lens_profile.ca_b.is_empty()),
        );
        let xform_now = (
            self.doc().recipe.straighten_deg,
            self.doc().recipe.lens_distortion,
            profile_geom,
            (self.doc().recipe.ca_r, self.doc().recipe.ca_b),
        );
        // The identity key, spelled ONCE (it grew a fourth member with the
        // manual CA pair in R25 B3, and it is compared in four places).
        const IDENTITY_XFORM: (f32, f32, (bool, bool), (f32, f32)) =
            (0.0, 0.0, (false, false), (0.0, 0.0));
        let stale_xform = workspace.overlay().mask_tex.is_some() && workspace.overlay().mask_tex_xform != xform_now;
        if workspace.overlay().mask_dirty || stale_xform {
            // Fast path (the common no-geometry case): the change since the
            // last upload is a known brush rect — upload ONLY that
            // sub-rectangle. Brushing used to clone and re-upload the WHOLE
            // canvas on every pointer move (at an 8192 working preview that
            // is a ~270 MB round trip per frame).
            if xform_now == IDENTITY_XFORM && !stale_xform && workspace.overlay().mask_dirty {
                let upload = {
                    let (m, tex, rect) = workspace.paint_upload_parts();
                    match (m, tex, rect) {
                        (Some(m), Some(tex), Some([x0, y0, x1, y1]))
                            if tex.size() == [m.width() as usize, m.height() as usize]
                                && x1 > x0
                                && y1 > y0
                                && x1 <= m.width()
                                && y1 <= m.height() =>
                        {
                            let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
                            let mut sub = Vec::with_capacity(w * h * 4);
                            for y in y0..y1 {
                                let start = ((y * m.width() + x0) * 4) as usize;
                                sub.extend_from_slice(&m.as_raw()[start..start + w * 4]);
                            }
                            Some((x0, y0, w, h, sub))
                        }
                        _ => None,
                    }
                };
                if let Some((x0, y0, w, h, sub)) = upload {
                    let overlay = workspace.overlay_mut();
                    if let Some(tex) = overlay.mask_tex.as_mut() {
                        tex.set_partial(
                            [x0 as usize, y0 as usize],
                            ColorImage::from_rgba_unmultiplied([w, h], &sub),
                            TextureOptions::LINEAR,
                        );
                    }
                    overlay.mask_dirty_rect = None;
                    overlay.mask_dirty = false;
                    return;
                }
            }
            // Geometry active: the remap is whole-frame work — mid-stroke,
            // rebuild at a bounded cadence instead of on every pointer move
            // (the stroke's final shape lands on the release frame, when the
            // pointer is no longer down).
            if !stale_xform
                && xform_now != IDENTITY_XFORM
                && workspace.overlay().mask_tex.is_some()
                && ctx.input(|i| i.pointer.any_down())
                && workspace.overlay().mask_tex_built.elapsed() < std::time::Duration::from_millis(120)
            {
                return; // mask_dirty stays armed; retried next frame
            }
            if let Some(m) = &workspace.masks().mask_paint {
                let ci = if xform_now != IDENTITY_XFORM {
                    // Alpha-preserving RGBA twins: the RGB16 photo paths
                    // flatten transparency to opaque, which turned the whole
                    // canvas into a red wash under any active geometry.
                    let mut img = m.clone();
                    if xform_now.1 != 0.0
                        || profile_geom != (false, false)
                        || xform_now.3 != (0.0, 0.0)
                    {
                        img = autoshade::render::apply_lens_geometry_rgba(
                            &img,
                            &autoshade::render::geometry_profile(&self.doc().recipe),
                            xform_now.1,
                        );
                    }
                    if xform_now.0 != 0.0 {
                        img = autoshade::render::rotate_straighten_rgba(&img, xform_now.0);
                    }
                    let rgba = img;
                    ColorImage::from_rgba_unmultiplied(
                        [rgba.width() as usize, rgba.height() as usize],
                        rgba.as_raw(),
                    )
                } else {
                    // Common case: no geometry — zero-copy view of the canvas.
                    ColorImage::from_rgba_unmultiplied(
                        [m.width() as usize, m.height() as usize],
                        m.as_raw(),
                    )
                };
                // Update in place: brushing sets mask_dirty on every pointer
                // move, and a fresh texture per frame is the allocate/free
                // churn finish_redevelop documents avoiding (set() re-uploads,
                // size changes included).
                if let Some(tex) = &mut workspace.overlay_mut().mask_tex {
                    tex.set(ci, TextureOptions::LINEAR);
                } else {
                    workspace.overlay_mut().mask_tex =
                        Some(ctx.load_texture("paintmask", ci, TextureOptions::LINEAR));
                }
                workspace.overlay_mut().mask_tex_xform = xform_now;
                workspace.overlay_mut().mask_tex_built = Instant::now();
            }
            workspace.overlay_mut().mask_dirty_rect = None; // full upload covered everything pending
            workspace.overlay_mut().mask_dirty = false;
        }
    }
}

impl Editor {
    /// Drop the open photo's decoded-base cache entries. Called whenever its
    /// BAKED pixel identity changes (a retouch lands, a save writes/clears
    /// pixels.json) — the source mtime doesn't move then, so the mtime guard
    /// alone would happily serve a hit with stale baked pixels.
    pub(crate) fn forget_open_base(&mut self, workspace: &mut WorkspaceView) {
        if let Some(p) = self.photo().src_path.clone() {
            crate::logic::lru::retain(&mut workspace.open_mut().base_cache, |(q, _, _, _)| q != &p);
        }
    }

    /// Every crop_mode flip goes through here (L13-1): `pan` is stored in
    /// the CURRENT window's coordinates — the committed crop normally, the
    /// full frame while the crop tool is open (`view_uv`) — so the VALUE
    /// must be rebased when the window changes. Left alone, entering the
    /// tool reinterpreted a crop-relative pan as full-frame and the
    /// viewport landed outside the crop box at higher zoom.
    pub(crate) fn set_crop_mode(&mut self, workspace: &mut WorkspaceView, on: bool) {
        if workspace.crop().mode == on {
            return;
        }
        if let Some(c) = &self.doc().recipe.crop {
            let (l, t) = (c.left.min(c.right), c.top.min(c.bottom));
            let (w, h) =
                ((c.right - c.left).abs().max(1e-6), (c.bottom - c.top).abs().max(1e-6));
            workspace.view_mut().pan = if on {
                // crop-window coords → full-frame coords
                vec2(l + workspace.view().pan.x * w, t + workspace.view().pan.y * h)
            } else {
                // full-frame coords → crop-window coords
                vec2((workspace.view().pan.x - l) / w, (workspace.view().pan.y - t) / h)
            };
            // view_uv re-clamps against zoom each frame; keep it sane here.
            workspace.view_mut().pan = vec2(workspace.view().pan.x.clamp(0.0, 1.0), workspace.view().pan.y.clamp(0.0, 1.0));
        }
        workspace.crop_mut().mode = on;
    }

    /// Commit a pending mask-rename buffer to its own mask (the same
    /// seed-guard the panel's row-switch commit uses: only while that mask's
    /// name still equals the seed-time snapshot). Focus-independent Ctrl+S
    /// runs while the TextEdit still holds focus — without this it saved the
    /// old name and the advanced baseline dropped the rename.
    pub(crate) fn commit_mask_name_buf(&mut self, workspace: &mut WorkspaceView) {
        if let Some((j, orig, buf)) = workspace.masks().mask_name_buf.clone()
            && let Some(prev) = self.doc_mut().recipe.masks.get_mut(j)
            && prev.name == orig
            && buf != orig
        {
            prev.name = buf.clone();
            // Re-seed so the panel's own lost-focus commit stays a no-op.
            workspace.masks_mut().mask_name_buf = Some((j, buf.clone(), buf));
        }
    }
}

impl Editor {
    /// Write the status for a door that just installed a canvas, in BOTH
    /// directions: the disclosure when this canvas disagrees with what the
    /// preference can deliver, and `plain` when it does not.
    ///
    /// The `plain` arm is not decoration. Announcing the disagreement without
    /// retracting it left the previous door's "stays at 1280px — edits and
    /// retouches follow that" standing after a redo had put the canvas back
    /// where the preference could reach: both halves false, on the status
    /// bar, which never expires. A door that can create the disagreement can
    /// also end it, so every door writes on every path.
    pub(crate) fn set_canvas_status(&mut self, workspace: &mut WorkspaceView, lang: Lang, plain: &'static str) {
                *workspace.status_mut() = match self.baked_canvas_edge(workspace) {
            Some(px) => trf(
                lang,
                K_CANVAS_THE_CANVAS_PIXELS_STAY_AT_PX_THEIR_OWN_BAKE_EDIT,
                &[("px", &px.to_string())],
            ),
            None => tr(lang, plain).to_string(),
        };
    }
}

impl Editor {
        pub(crate) fn source_is_raw(&self) -> bool {
        self.active_source_path().is_some_and(|p| autoshade::decode::is_raw(&p))
    }
}

impl Editor {
    /// SF8-A: hand the stored Lightroom sidecar over to Lightroom — copy this
    /// photo's `<stem>.xmp` projection out of the hashed develop folder and
    /// beside the photo itself, the only place Lightroom looks.
    ///
    /// Two-click confirm rather than a modal: an existing `.xmp` beside the
    /// photo may be Lightroom's OWN, so the first click arms
    /// `xmp_beside_confirm` (the button relabels itself) and only the second
    /// passes `overwrite = true`. The refusal comes from the store's atomic
    /// claim, not from an `exists()` probe here, so the arming can never be
    /// answered by a file that appeared in between.
    pub(crate) fn export_xmp_beside(&mut self, workspace: &mut WorkspaceView, lang: Lang) {
                let Some(path) = self.photo().src_path.clone() else { return };
        let overwrite = workspace.exp().xmp_beside_confirm;
        match autoshade::store::export_xmp_beside(&path, overwrite) {
            Ok(to) => {
                workspace.exp_mut().xmp_beside_confirm = false;
                // A file just landed in the user's PHOTO folder — the one place
                // this app otherwise never writes. That is worth a toast, not
                // just a status line that the next message scrolls away.
                let mut t = trf(
                    lang,
                    K_EXPORT_LIGHTROOM_SIDECAR_DELIVERED,
                    &[("path", &abs_display(&to))],
                );
                // The file handed over is the projection of the SAVED develop,
                // so its global half is said again here, where the user is
                // about to open it in Lightroom (a colour field, a lens blur:
                // the rendered exports carry them, the sidecar cannot). The
                // mask half was said on the save that wrote the file.
                let globals = autoshade::xmp::global_export_losses(&self.session().hist.saved_recipe);
                if let Some(m) = xmp_loss_line(lang, &[], &globals)
                    && xmp_loss_interrupts(&[], &globals)
                {
                    workspace.toast(ToastKind::Error, m.clone());
                    t = format!("{t} — ⚠ {m}");
                }
                workspace.set_status(t.clone());
                workspace.toast(ToastKind::Success, t);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && !overwrite => {
                // ARM, do not write. The status says what the second click will
                // do; the button says it too (develop.rs) so the armed state is
                // visible even after the status line has moved on.
                workspace.exp_mut().xmp_beside_confirm = true;
                let there = autoshade::store::xmp_beside_target(&path)
                    .map(|p| abs_display(&p))
                    .unwrap_or_default();
                let t = trf(
                    lang,
                    K_EXPORT_A_XMP_ALREADY_SITS_BESIDE_THIS_PHOTO_CLICK_AGAIN,
                    &[("path", &there)],
                );
                workspace.set_status(t.clone());
                workspace.toast(ToastKind::Error, t);
            }
            Err(e) => {
                workspace.exp_mut().xmp_beside_confirm = false;
                let t = trf(
                    lang,
                    K_EXPORT_THE_XMP_COULD_NOT_BE_DELIVERED,
                    &[("err", &e.to_string())],
                );
                workspace.set_status(t.clone());
                workspace.toast(ToastKind::Error, t);
            }
        }
    }
}

impl Editor {
    /// Move one mask a row up/down. Mask ORDER is render semantics (masks
    /// stack sequentially; a later mask's range sees earlier masks' output), so
    /// the list order is editable — and the armed-tool indices remap with it.
    pub(crate) fn move_mask(&mut self, workspace: &mut WorkspaceView, i: usize, up: bool) {
        let j = if up {
            if i == 0 { return }
            i - 1
        } else {
            if i + 1 >= self.doc().recipe.masks.len() { return }
            i + 1
        };
        crate::logic::command::apply(
            &mut self.doc_mut().recipe,
            crate::logic::command::Command::Mask(if up {
                crate::logic::command::MaskCommand::MoveUp { index: i }
            } else {
                crate::logic::command::MaskCommand::MoveDown { index: i }
            }),
        );
        workspace.remap_mask_indices(|s| Some(if s == i { j } else if s == j { i } else { s }));
        workspace.overlay_mut().overlay_stale = true;
        self.session_mut().dirty = true;
    }

    /// Paint the imported removals ADOBE SYNTHESISED into Generative Fill's
    /// painted area, so the generative verb can be run over exactly them and
    /// nothing else (v1.5.0 F9, the second half of the user's ruling: local repair at
    /// import, generative re-solve on request).
    ///
    /// Returns how many areas were painted, so the caller can stay silent when
    /// there is nothing to do rather than spawn a worker over a blank mask.
    ///
    /// Writes the DUAL PAIR — the red display canvas and the greyscale weight
    /// buffer — because that is the session contract `clear_mask` documents:
    /// the brush writes both, Apply bakes the grey, and a surface that wrote
    /// only the canvas would show strokes that bake to nothing.
    ///
    /// The exact painted SHAPE is used wherever the area has one. A brush area
    /// arrives as `SpotCoverage` (its dabs, rasterised), and asking a model to
    /// regenerate the enclosing disk of a long thin stroke would rewrite a
    /// circle of content the photographer never touched.
    pub(crate) fn paint_imported_removals(&mut self, workspace: &mut WorkspaceView) -> usize {
        let Some(base) = self.photo().base_preview.as_ref() else { return 0 };
        let (mw, mh) = base.dimensions();
        if mw == 0 || mh == 0 {
            return 0;
        }
        let wanted: Vec<_> =
            autoshade::render::retouch_spots(&self.doc().recipe, mw as usize, mh as usize)
                .into_iter()
                .filter(|s| s.origin.is_synthesised())
                .collect();
        if wanted.is_empty() {
            return 0;
        }
        // Fill's own area — the verb these shapes are painted for.
        workspace.select_brush_owner(BrushOwner::Fill);
        // At the PREVIEW's dimensions, which is the frame the brush paints in
        // and the frame `export_mask_png` hands on.
        if workspace.masks().mask_paint.as_ref().is_none_or(|m| m.dimensions() != (mw, mh)) {
            workspace.masks_mut().mask_paint = Some(image::RgbaImage::new(mw, mh));
            workspace.paint_mask_changed(Some(false));
        }
        if workspace.masks().mask_brush_gray.as_ref().is_none_or(|g| g.dimensions() != (mw, mh)) {
            workspace.masks_mut().mask_brush_gray = Some(image::GrayImage::new(mw, mh));
        }
        let masks = workspace.masks_mut();
        let canvas = masks.mask_paint.as_mut().expect("just ensured");
        let gray = masks.mask_brush_gray.as_mut().expect("just ensured");
        let short = mw.min(mh) as f32;
        for s in &wanted {
            let c = (s.cx * mw as f32, s.cy * mh as f32);
            let r = (s.radius * short).max(1.0);
            match &s.coverage {
                Some(cov) => {
                    // Walk the enclosing disk's box and keep only what the
                    // stroke actually covers.
                    let x0 = (c.0 - r).floor().max(0.0) as u32;
                    let y0 = (c.1 - r).floor().max(0.0) as u32;
                    let x1 = ((c.0 + r).ceil().max(0.0) as u32).min(mw - 1);
                    let y1 = ((c.1 + r).ceil().max(0.0) as u32).min(mh - 1);
                    for y in y0..=y1 {
                        for x in x0..=x1 {
                            let nx = (x as f32 + 0.5) / mw as f32;
                            let ny = (y as f32 + 0.5) / mh as f32;
                            if cov.weight_at(nx, ny) > 0.5 {
                                canvas.put_pixel(x, y, image::Rgba([255, 64, 64, 160]));
                                gray.put_pixel(x, y, image::Luma([255]));
                            }
                        }
                    }
                }
                // An ellipse IS its disk — 84 of the library's 121 areas.
                None => {
                    stamp_dot_px(canvas, c, r, image::Rgba([255, 64, 64, 160]), BrushTip::HARD);
                    stamp_dot_gray(gray, c, r, 255, BrushTip::HARD);
                }
            }
        }
        workspace.paint_mask_changed(None);
        // The whole canvas changed, so a pending sub-rect from an earlier
        // stroke must not turn this into a partial GPU upload (see clear_mask).
        workspace.overlay_mut().mask_dirty_rect = None;
        wanted.len()
    }
}

impl Editor {
    /// Where this photo's base negative sits in the strip — `None` when the
    /// strip has none. Not a hypothetical: a cold restore whose record named
    /// a `fitted` active card and listed no Original entry rebuilds exactly
    /// that, so every caller must say something honest instead of assuming
    /// card 0 is the negative.
    pub(crate) fn original_index(&self) -> Option<usize> {
        crate::logic::cards::original_index(&self.doc().variants)
    }
}

impl Editor {
    /// Commit a pending version rename to ITS OWN photo and number — the
    /// version half of the `commit_mask_name_buf` rule (U10): a name still
    /// sitting in its TextEdit when the user hits Ctrl+S, clicks another
    /// photo or quits must reach the disk, not die with the buffer. Keyed by
    /// (photo, number), both captured at seed time, so a boundary that has
    /// already re-pointed `src_path` cannot rename the incoming photo's v1.
    /// The write is advisory: a failure is disclosed and the buffer is left
    /// alone so the next boundary retries.
    pub(crate) fn commit_version_name_buf(&mut self, workspace: &mut WorkspaceView, lang: Lang) {
        let Some((photo, n, seed, buf)) = workspace.cards().version_name_buf.clone() else { return };
        if buf == seed {
            return;
        }
        // NoWait wrapper around the store's own Wait lock (the delete_version
        // rule): a develop held by another process must report here, never
        // freeze the UI thread on a name.
        match autoshade::store::with_develop_lock(
            &photo,
            autoshade::store::DevelopLockMode::NoWait,
            || autoshade::store::set_version_name(&photo, n, Some(buf.as_str())),
        ) {
            Ok(()) => {
                let named = Some(buf.trim().to_string()).filter(|s| !s.is_empty());
                // Re-seed rather than clear: the row is very likely still on
                // screen, and a SECOND rename in the same row has to compare
                // against what we just wrote, not the original.
                workspace.cards_mut().version_name_buf = Some((photo.clone(), n, buf.clone(), buf));
                if self.photo().src_path.as_deref() == Some(photo.as_path()) {
                    workspace.cards_mut().version_meta
                        .entry(n)
                        .or_insert_with(|| autoshade::store::VersionMetaEntry {
                            n,
                            ..Default::default()
                        })
                        .name = named;
                }
            }
            Err(e) => {
                workspace.persist_postponed(lang, &e, "Renaming v{n} failed: {err}", &[("n", &n.to_string())]);
            }
        }
    }
}

impl Editor {
    /// The full-resolution SHORT EDGE of the active card's pixel source — the
    /// film the Detail panel states its radii in (`render::FilmScale`,
    /// v1.5.0), so the canvas shows sharpening and noise reduction the way the
    /// export looks once it is downscaled to the canvas.
    ///
    /// A header read (`decode::film_short_edge`), cached against the path AND its
    /// modification time: the preview asks on every slider tick, and the
    /// answer changes only when the source does — a re-baked raster at the
    /// same path included. `None` (no photo, an unreadable header) develops the
    /// canvas as its own film, which is what every build before v1.5.0 did.
    pub(crate) fn film_short_edge(&mut self) -> Option<u32> {
        let path = self.active_source_path()?;
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if let Some((cached, when, edge)) = &self.photo().film_edge
            && *cached == path
            && *when == modified
        {
            return *edge;
        }
        let edge = autoshade::decode::film_short_edge(&path);
        self.photo_mut().film_edge = Some((path, modified, edge));
        edge
    }

    /// The frame of the active card's pixel source (v1.17.0): a RAW, or a
    /// baked file with the lens geometry its own marker says is in it
    /// (`render::SourceFrame::of`, cached per file for the process).
    pub(crate) fn source_frame(&self) -> anyhow::Result<autoshade::render::SourceFrame> {
        match self.active_source_path() {
            Some(p) => autoshade::render::SourceFrame::of(&p),
            None => Ok(autoshade::render::SourceFrame::Baked(None)),
        }
    }

    /// Is the canvas base in the lens-CORRECTED frame (v1.17.0)? A RAW source's
    /// base never is; a baked master is when its own marker says so
    /// (`render::SourceFrame`). A source
    /// whose frame cannot be read is not drawn at all (`start_redevelop`).
    pub(crate) fn base_corrected(&self) -> bool {
        self.source_frame().is_ok_and(|f| f.corrected())
    }

    /// An ORIGINAL-frame point → the point on the canvas base that shows the
    /// same content: itself, or where the lens correction put it.
    pub(crate) fn orig_to_base_norm(&self, (nx, ny): (f32, f32)) -> (f32, f32) {
        if !self.base_corrected() {
            return (nx, ny);
        }
        let (dims, _, dist) = self.geom_ctx();
        autoshade::render::lens_ungeom_norm(nx, ny, dims, &dist.profile, dist.amount)
    }

    /// The mask frame of a SAMPLED develop over the canvas base (no geometry
    /// stage follows): `MaskFrame::Corrected` on a corrected base, else
    /// `MaskFrame::AsRendered`.
    pub(crate) fn sampled_frame<'a>(&self, composed: &'a autoshade::recipe::LensProfile) -> autoshade::render::MaskFrame<'a> {
        if self.base_corrected() {
            autoshade::render::MaskFrame::Corrected { profile: composed }
        } else {
            autoshade::render::MaskFrame::AsRendered
        }
    }

    /// The canvas base in the ORIGINAL frame, for the helpers that draw a
    /// raster the recipe stores there (segmentation, depth, the face find): a
    /// corrected base mapped back through the engine's own inverse
    /// (`render::corrected_to_sensor_frame`), otherwise the base itself.
    pub(crate) fn original_frame_base(&self) -> Option<Arc<image::DynamicImage>> {
        let base = self.photo().base_preview.clone()?;
        if !self.base_corrected() {
            return Some(base);
        }
        let geom = autoshade::render::geometry_profile(&self.doc().recipe);
        Some(match autoshade::render::corrected_to_sensor_frame(&base, &geom, 0.0) {
            Some((img, _)) => Arc::new(image::DynamicImage::ImageRgb8(img.to_rgb8())),
            None => base,
        })
    }
}

impl Editor {
    /// The key the portrait skin switch is filed under: the photo and the
    /// active card's id. `None` with no photo open.
    pub(crate) fn skin_key(&self) -> Option<(PathBuf, String)> {
        let photo = self.photo().src_path.clone()?;
        Some((photo, self.doc().variants.get(self.doc().active).map(|v| v.id.clone()).unwrap_or_default()))
    }

    /// The skin switch of the ACTIVE card — on by default, and on for a card
    /// with no entry (a fresh card is inside the limits by construction).
    pub(crate) fn keep_skin_on(&self, workspace: &WorkspaceView) -> bool {
        self.skin_key().and_then(|k| workspace.portrait().keep_skin.get(&k).copied()).unwrap_or(true)
    }

    /// The skin switch as a HISTORY STEP sees it: `None` when there is no
    /// card to file it under (no photo), so an undo step can tell "on" from
    /// "no opinion".
    pub(crate) fn skin_switch(&self, workspace: &WorkspaceView) -> Option<bool> {
        self.skin_key().map(|_| self.keep_skin_on(workspace))
    }

    pub(crate) fn skin_held(&self, workspace: &WorkspaceView) -> bool {
        self.keep_skin_on(workspace)
    }

    /// Is the skin switch held for `photo`? For a photo that is NOT open the
    /// answer comes from its stashed nav record — the switch belongs to a
    /// (photo, card) pair, not to the canvas that happens to be showing.
    pub(crate) fn skin_held_for(&self, workspace: &WorkspaceView, photo: &std::path::Path) -> bool {
        if self.photo().src_path.as_deref() == Some(photo) {
            return self.skin_held(workspace);
        }
        workspace.open().nav_stash.get(photo).is_none_or(|st| {
            workspace.portrait().keep_skin.get(&(photo.to_path_buf(), st.id.clone())).copied().unwrap_or(true)
        })
    }

    /// A history step brought back an edit past the skin limits: file the
    /// switch as OFF rather than leave it claiming "natural skin" it no
    /// longer holds.
    pub(crate) fn release_skin_for_history(&mut self, workspace: &mut WorkspaceView) {
        if self.skin_held(workspace)
            && autoshade::face::skin_values_beyond(&self.doc().recipe) > 0
            && let Some(key) = self.skin_key()
        {
            workspace.portrait_mut().keep_skin.insert(key, false);
        }
    }

    /// Hand the skin switch back as the undone step had it.
    pub(crate) fn restore_skin_switch(&mut self, workspace: &mut WorkspaceView, on: Option<bool>) {
        if let (Some(on), Some(key)) = (on, self.skin_key()) {
            workspace.portrait_mut().keep_skin.insert(key, on);
        }
    }

    /// The live working state as an [`UndoStep`]: current recipe + the active
    /// variant's pixel identity. Arc/path clones only — never pixel copies.
    pub(crate) fn current_step(&self, workspace: &WorkspaceView) -> UndoStep {
        UndoStep {
            recipe: self.doc().recipe.clone(),
            base: self.active_variant().and_then(|v| v.base.clone()),
            origin: self.active_variant().and_then(|v| v.origin.clone()),
            keep_skin: self.skin_switch(workspace),
        }
    }

    /// Reset undo history — call when a brand-new photo opens, when the active
    /// variant changes (you can't undo across either), or when a photo GOES
    /// AWAY: a failed fresh open leaves no canvas for its steps to apply to,
    /// and an undo there restored one that no longer existed. `committed`
    /// becomes the current head — with no photo open that is the empty step,
    /// which is exactly what stops the next settled frame pushing a phantom.
    pub(crate) fn reset_history(&mut self, workspace: &WorkspaceView) {
        let head = self.current_step(workspace);
        let hist = self.history_mut();
        hist.committed = head;
        hist.undo_stack.clear();
        hist.redo_stack.clear();
    }

    /// Commit the current state as one undo step RIGHT NOW (when it differs
    /// from the head). Used directly by programmatic pixel swaps (the retouch
    /// bake-in): waiting for `commit_if_settled` left a one-frame window where
    /// a same-frame Ctrl+Z saw only the older history and undid a recipe step
    /// while keeping the freshly retouched pixels installed.
    pub(crate) fn commit_now(&mut self, workspace: &WorkspaceView) {
        let cur = self.current_step(workspace);
        let hist = self.history_mut();
        if cur.recipe != hist.committed.recipe || !cur.same_pixels(&hist.committed) {
            hist.undo_stack.push(std::mem::replace(&mut hist.committed, cur));
            if hist.undo_stack.len() > 100 {
                hist.undo_stack.remove(0); // cap history memory
            }
            hist.redo_stack.clear();
        } else {
            // A flip of the skin switch that changed no value belongs to the
            // step on screen, so a later undo/redo hands it back as set.
            hist.committed.keep_skin = cur.keep_skin;
        }
    }

    /// Commit the current state as ONE undo step once the edit gesture settles
    /// (pointer released) — dragging a slider is one step, not one per frame.
    /// Programmatic edits (Analyze, Reset) land here on the next frame.
    pub(crate) fn commit_if_settled(&mut self, workspace: &WorkspaceView, ctx: &Ctx) {
        if !ctx.input(|i| i.pointer.any_down()) {
            self.commit_now(workspace);
        }
    }
}
