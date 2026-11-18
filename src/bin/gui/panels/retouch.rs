//! Retouch panel: paint/clone/fill/heal/AI tools.

use crate::*;

/// The brush radius range, in After-image display px. The ceiling was 80 —
/// a 160 px disc, too small to paint a sky on a large screen in one pass.
pub(crate) const BRUSH_R_MIN: f32 = 4.0;
pub(crate) const BRUSH_R_MAX: f32 = 200.0;
/// How long the ring stays up, with its size, after the size changes.
const BRUSH_SIZE_FLASH_S: f64 = 1.2;

impl Commands<'_> {
    /// Brush-paint into the mask while dragging on the After image. The canvas
    /// is full-frame at preview resolution; pointer→canvas goes through the
    /// view transform so painting stays accurate at any zoom, and the brush
    /// radius converts display px → canvas px by the current pixel scale.
    pub(crate) fn handle_paint(&mut self, resp: &egui::Response, xf: ViewXform) {
        let brush = self.workspace.masks().brush;
        let tip = self.workspace.brush_tip();
        let (dims, deg, dist) = self.editor.geom_ctx();
        // Mask-brush session: strokes write the display canvas AND the
        // greyscale weight buffer; erase clears both.
        let erase = self.workspace.masks().mask_brush.is_some_and(|(_, e)| e);
        let display_px = if erase {
            image::Rgba([0, 0, 0, 0])
        } else {
            image::Rgba([255, 64, 64, 160])
        };
        let gray_v: u8 = if erase { 0 } else { 255 };
        let mut gray = self.workspace.masks_mut().mask_brush_gray.take();
        let (masks, overlay) = self.workspace.brush_parts();
        let Some(m) = masks.mask_paint.as_mut() else {
            masks.mask_brush_gray = gray;
            return;
        };
        let (mw, mh) = (m.width() as f32, m.height() as f32);
        // The canvas lives in the ORIGINAL frame (fill/heal edit source
        // pixels), so pointer positions map out of the transformed view.
        let to_mask = |p: egui::Pos2| {
            let (nx, ny) = xf.to_norm(p);
            let (ox, oy) = view_norm_to_orig(nx, ny, dims, deg, &dist);
            (ox * mw, oy * mh)
        };
        // Brush radius in MASK pixels, measured through the SAME nonlinear
        // view→original chain the centers use: map the pointer and a point one
        // display-radius to its right, take their mask-space distance. Under
        // straighten/distortion a fixed display brush covers a position-
        // dependent original-frame extent — the old constant horizontal-UV
        // conversion mismatched the painted footprint there. Neutral geometry
        // degenerates to exactly the old constant.
        let brush_at = |p: egui::Pos2| {
            let a = to_mask(p);
            // Probe BOTH directions and keep the larger. `ViewXform::to_norm`
            // CLAMPS to the viewport, so a rightward-only probe mapped to the
            // same point as the centre once the pointer reached the right
            // edge: the distance went to zero, the `.max(1.0)` floor took
            // over, and a 30 px brush stamped a single mask pixel exactly
            // where the user was painting along that edge. The chain is
            // smooth, so the direction with room measures the same radius.
            let probe = |dx: f32| {
                let b = to_mask(p + egui::vec2(dx, 0.0));
                ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
            };
            probe(brush).max(probe(-brush)).max(1.0)
        };
        // Union a brush segment's bounding box (± radius, canvas px) into the
        // pending dirty rect — ensure_mask_tex's partial-upload path.
        let grow = |rect: &mut Option<[u32; 4]>, a: (f32, f32), b: (f32, f32), r: f32| {
            let r = r + 1.0;
            let n = [
                (a.0.min(b.0) - r).floor().clamp(0.0, mw) as u32,
                (a.1.min(b.1) - r).floor().clamp(0.0, mh) as u32,
                (a.0.max(b.0) + r).ceil().clamp(0.0, mw) as u32,
                (a.1.max(b.1) + r).ceil().clamp(0.0, mh) as u32,
            ];
            *rect = Some(match rect {
                Some(o) => [o[0].min(n[0]), o[1].min(n[1]), o[2].max(n[2]), o[3].max(n[3])],
                None => n,
            });
        };
        let mut stamped = false;
        // Ink from the moment the PRIMARY button goes down on the canvas
        // (2026-10-13, user: 「操作感觉也不连贯」): egui reports a drag only
        // once the pointer has travelled ~6 px, and a press that stayed put
        // only as a click on RELEASE, so a stroke used to start late and a dab
        // appeared only when the button came up. Primary only — a secondary
        // or middle drag is the caller's pan — and never with Space held, the
        // pan chord, whose press frame comes before the caller sees a drag.
        let down = resp.is_pointer_button_down_on()
            && resp.ctx.input(|i| i.pointer.primary_down() && !i.key_down(egui::Key::Space));
        if down {
            if let Some(p) = resp.interact_pointer_pos().or(resp.hover_pos()) {
                let cur = to_mask(p);
                let r = brush_at(p);
                match masks.paint_last {
                    Some(prev) => {
                        stamped = stamp_line_px(m, prev, cur, r, display_px, tip);
                        if let Some(g) = gray.as_mut() {
                            stamp_line_gray(g, prev, cur, r, gray_v, tip);
                        }
                    }
                    // First stroke frame: from the PRESS ORIGIN (on the press
                    // frame itself that is the pointer, so a still press
                    // stamps one dab at once).
                    None => {
                        let start = ui_press_origin(resp).map(&to_mask).unwrap_or(cur);
                        stamped = stamp_line_px(m, start, cur, r, display_px, tip);
                        if let Some(g) = gray.as_mut() {
                            stamp_line_gray(g, start, cur, r, gray_v, tip);
                        }
                        grow(&mut overlay.mask_dirty_rect, start, cur, r);
                    }
                }
                grow(&mut overlay.mask_dirty_rect, masks.paint_last.unwrap_or(cur), cur, r);
                masks.paint_last = Some(cur);
            }
        } else {
            // No stroke is in flight while the button is up — clearing here
            // (not just on drag_stopped) also covers strokes interrupted by
            // Esc / hold-B / an Alt source-pick, whose drag_stopped frame
            // never reaches this handler and used to leave a stale anchor
            // that drew a full-width connecting streak into the next stroke.
            masks.paint_last = None;
        }
        masks.mask_brush_gray = gray;
        if stamped {
            if erase {
                self.workspace.paint_mask_changed(None);
            } else {
                self.workspace.paint_mask_changed(Some(true));
            }
        }
    }

    /// Clone-stamp interaction: Alt+click picks the SOURCE point (stored in
    /// the original frame like every pixel-path coordinate); plain drags paint
    /// the target into the Stamp's own area. The picked source stays marked
    /// with a crosshair ring so the offset is always visible.
    pub(crate) fn handle_clone(&mut self, ui: &egui::Ui, resp: &egui::Response, xf: ViewXform) {
        let alt = ui.input(|i| i.modifiers.alt);
        if alt {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            if resp.clicked()
                && let Some(q) = resp.interact_pointer_pos()
            {
                let (dims, deg, dist) = self.editor.geom_ctx();
                let (nx, ny) = xf.to_norm(q);
                self.workspace.tools_mut().clone_src = Some(view_norm_to_orig(nx, ny, dims, deg, &dist));
                *self.workspace.status_mut() = tr(
                    self.lang,
                    K_RETOUCH_CLONE_SOURCE_SAMPLED_BRUSH_THE_AREA_TO_COVER_THE,
                )
                .into();
            }
        } else {
            self.handle_paint(resp, xf);
        }
        if let Some((sx, sy)) = self.workspace.tools().clone_src {
            let (dims, deg, dist) = self.editor.geom_ctx();
            let (vx, vy) = orig_norm_to_view(sx, sy, dims, deg, &dist);
            let q = xf.to_screen(vx, vy);
            let p = ui.painter_at(xf.rect);
            // ACCENT, per the two-tier colour rule: on-canvas tool overlays
            // never wear the PILL chrome gold — a source pin sampled on skin,
            // sand or a sunset sky simply vanished in gold.
            p.circle_stroke(q, 9.0, egui::Stroke::new(2.0, ACCENT));
            p.line_segment(
                [q - egui::vec2(13.0, 0.0), q + egui::vec2(13.0, 0.0)],
                egui::Stroke::new(1.0, ACCENT),
            );
            p.line_segment(
                [q - egui::vec2(0.0, 13.0), q + egui::vec2(0.0, 13.0)],
                egui::Stroke::new(1.0, ACCENT),
            );
        }
    }

    /// R27 — every pixel worker takes this gate: heal, fill, adjust, clone,
    /// AI denoise, stack and reimagine. They take the PHOTO PATH and
    /// re-develop it through `render::source_pixels`, which applies the EXIF
    /// orientation and NOTHING else, while the canvas is in the frame
    /// `recipe.quarter_turns` produces. On a turned photo those are different
    /// frames, and each door suffers it in its own way:
    ///   * the masked four (heal, fill, adjust, clone) export their mask from
    ///     the turned canvas, so the repair lands on the wrong pixels — BAKED,
    ///     into a master no slider can undo;
    ///   * the mask-free three (AI denoise, stack, reimagine) land their
    ///     master as a ◈/▦/✨ card whose raster is in the EXIF frame:
    ///     `load_active` installs it as the plate, and `sync_base_turns`
    ///     cannot turn a baked card — the canvas shows it sideways.
    ///
    /// Refused with a reason rather than approximated. Threading the turn
    /// through `source_pixels` is its own change: that function is THE
    /// raw-vs-baked dispatch and denoise / match / reverse-fit share it.
    /// Registered limitation (ROADMAP, R27 Batch-2 ②).
    ///
    /// The AI mask refine (`masks.rs`) takes it too, since 2026-09-24: its
    /// guide is decoded in the EXIF frame and the raster it refines was
    /// turned into the plate frame.
    ///
    /// Returns `true` when the caller must stop.
    pub(crate) fn refuse_pixel_work_on_a_turned_photo(&mut self) -> bool {
        if self.editor.doc().recipe.quarter_turns.is_multiple_of(4) {
            return false;
        }
        *self.workspace.status_mut() = tr(
            self.lang,
            K_RETOUCH_THIS_PHOTO_IS_TURNED_AND_PIXEL_REPAIRS_STILL_WOR,
        )
        .into();
        true
    }

    /// Generative fill: the model is shown THIS card's developed picture —
    /// the card's own pixel source under its live recipe, in the original
    /// frame the mask is painted in (geometry is a view, not a pixel) — the
    /// painted area is regenerated from the prompt (blank = remove), and the
    /// result lands as a NEW「✨ AI generated」card whose look lives in its
    /// pixels; the card you filled from is untouched. Heal / clone / denoise
    /// stay in-place touch-ups of the neutral master: they are pixel
    /// arithmetic, not a model that has to see the picture. Through v1.3.4
    /// this too was in-place on the neutral base, so the model was shown a
    /// develop 0.6–1.4 EV under the card's Before with none of its sliders
    /// or masks, and the card's content-keyed corrections then landed on
    /// content they were never solved for (user, 2026-09-15: 「不是基于当前
    /// 变体生成的，而是原图」). Runs on a worker thread.
        /// Edit the active generated card's live picture. The fold's own painted
    /// area (`BrushOwner::Adjust`) chooses the region path at the click; blank
    /// means remove there.
    /// Every answer is a new ✨ card, so the source stays immutable and the
    /// result can be adjusted again or reverse-fit like any generated card.
        /// Heal: AI auto-detect (use_mask=false) or the painted mask (use_mask=true).
    /// Pixel retouch from surrounding real pixels; saves to ./out. `limit` is
    /// a mask raster the heal is held to (`retouch::heal`'s limit): the
    /// Portraits panel's 🤖 Remove blemishes passes the face mask's; the
    /// Retouch panel passes none.
        /// AI denoise as an ACTIVE canvas operation (2026-09-15): denoise the
    /// current card's pixel source at full resolution — a RAW on its sensor
    /// mosaic before demosaic, a baked source through SCUNet — and land the
    /// master as a NEW ◈ Denoised card (`RetouchKind::NewDenoised`) that
    /// carries this card's develop. The card you started from, the ▣
    /// negative included, keeps its pixels; the ◈ card is deleted, not
    /// undone. The ≤2048 px working-copy tier retired with the in-place
    /// landing: a master baked from it capped every later export at 2048 px.
        /// Stack this card's frame with others (v1.5.0 Track S).
    ///
    /// The ACTIVE card's source is frame 0 — the reference, whose framing and
    /// whose exposure the merge keeps (`stack::merge::merge`) — and `extra`
    /// are the frames the user picked beside it. The merged master lands as a
    /// NEW ▦ card carrying this card's develop, on exactly the ◈ denoise
    /// card's terms: the frame you started from keeps its own pixels.
    ///
    /// Full resolution, always, for the denoise landing's reason: a master
    /// baked from a working copy caps every later export at that size.
        /// Run the clone stamp on a worker: painted target mask + the Alt+picked
    /// source point → `retouch::clone_stamp` (deterministic, no AI) → ./out
    /// pixel master shown in the After pane, exactly like heal.
        /// Full-frame generative re-render via gpt-image — the OPTIONAL "let GPT
    /// directly make the picture" path. Uses the Direction text as the look
    /// prompt. Unlike Analyze (a faithful parametric recipe), this REGENERATES
    /// pixels — a creative restyle (up to ~8 MP on flexible-size models, ~1.5K
    /// on older ones). The result enters the strip as a new「AI 生成」variant
    /// (`origin = Some(out)`); its saved path is the reverse-fit ("反推配方")
    /// target that turns the look back into sliders + XMP at full resolution.
        /// Extract a reusable STYLE PROMPT from the before/after pair via the vision
    /// model. The result lands in the Direction box (ready to reimagine OTHER
    /// photos with the same look) and is saved to ./out/<stem>.style.txt.
        /// The brush-radius slider, rendered wherever a brush is actually IN USE
    /// (#9). There is exactly ONE radius in the app — `self.workspace.masks().brush`, also
    /// driven by the `[` / `]` keys (see the shortcut in app.rs) — and until
    /// R22 its only slider lived in the Retouch panel, beside Fill / Heal /
    /// Stamp. The mask brush, which starts from Develop → Local Masks and
    /// paints in its own session, had no size control at its own site at all:
    /// the widget stayed with its historical host instead of following the
    /// tool. Both renders bind this same field on purpose (two live sliders
    /// over one value is house precedent — global and per-mask Exposure), so
    /// the tooltip discloses the sharing rather than pretending there are two
    /// brushes.
    /// The brush's footprint where it will land (2026-10-13, user: 「点了之后
    /// 不知道大小」): a ring of the brush's radius under the pointer — dark
    /// outside, light inside, so it reads on sky and on shadow alike — with
    /// a centre dot, in place of the OS cursor. For a second after the size
    /// changes ([ / ] or the slider) the ring also shows with the size beside
    /// it, at the canvas centre when the pointer is off the canvas. Erasing
    /// marks the centre with a minus.
    pub(crate) fn draw_brush_cursor(&self, ui: &egui::Ui, resp: &egui::Response, rect: egui::Rect) {
        let now = ui.input(|i| i.time);
        let resized = self.workspace.masks().brush_resized_at.is_some_and(|t| now - t < BRUSH_SIZE_FLASH_S);
        let Some(c) = resp.hover_pos().or_else(|| resized.then(|| rect.center())) else { return };
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        }
        let p = ui.painter_at(rect);
        let r = self.workspace.masks().brush;
        p.circle_stroke(c, r, egui::Stroke::new(3.0, egui::Color32::from_black_alpha(150)));
        p.circle_stroke(c, r, egui::Stroke::new(1.25, egui::Color32::from_white_alpha(235)));
        // Lightroom's second ring: where the feather starts to fade.
        let inner = r * (1.0 - self.workspace.masks().brush_feather);
        if self.workspace.masks().brush_feather > 0.005 && inner > 2.0 {
            p.circle_stroke(c, inner, egui::Stroke::new(1.0, egui::Color32::from_white_alpha(140)));
        }
        if self.workspace.masks().mask_brush.is_some_and(|(_, erase)| erase) {
            let s = (r * 0.35).clamp(3.0, 8.0);
            p.line_segment([c - egui::vec2(s, 0.0), c + egui::vec2(s, 0.0)], egui::Stroke::new(1.5, egui::Color32::WHITE));
        } else {
            p.circle_filled(c, 1.5, egui::Color32::WHITE);
        }
        if resized {
            let label = format!("{} {:.0}", tr(self.lang, K_RETOUCH_BRUSH_SIZE), r);
            let at = c + egui::vec2(0.0, r + 8.0);
            let font = egui::TextStyle::Small.resolve(ui.style());
            p.text(at + egui::vec2(1.0, 1.0), egui::Align2::CENTER_TOP, &label, font.clone(), egui::Color32::from_black_alpha(200));
            p.text(at, egui::Align2::CENTER_TOP, &label, font, egui::Color32::WHITE);
            // Keep frames coming until the flash has run out.
            ui.ctx().request_repaint();
        }
    }




    /// ONE tool's brush controls, drawn inside that tool's fold (user
    /// decision 2026-09-27: the brush a fold's verb reads sits in that fold,
    /// and each fold has a painted area of its own — `BrushOwner`). The arm
    /// toggle and the clear verb share a two-cell row; the radius slider
    /// follows while this tool's brush is armed. The Clone Stamp arms through
    /// its own 「⎘ Enter stamp」 toggle and lays its row itself.
    pub(crate) fn brush_row(&mut self, ui: &mut egui::Ui, owner: BrushOwner) {
        let lang = self.lang;
        let armed = self.workspace.brush_armed(owner);
        ui.horizontal(|ui| {
            let cell = columns(ui, 2);
            if toggle_in(ui, cell, armed, tr(lang, K_RETOUCH_PAINT_AREA))
                .on_hover_text(tr(lang, K_RETOUCH_BRUSH_OVER_THE_AREA_THIS_TOOL_WORKS_ON_BOX_SELEC))
                .clicked()
            {
                if armed {
                    self.workspace.disarm_brush();
                } else {
                    self.arm_brush(owner);
                }
            }
            if action_in(ui, cell, true, tr(lang, K_RETOUCH_CLEAR_AREA))
                .on_hover_text(tr(lang, K_RETOUCH_WIPE_THIS_TOOL_S_PAINTED_AREA_THE_OTHER_TOOLS_AR))
                .clicked()
            {
                self.workspace.clear_area(owner);
            }
        });
        if armed {
            self.with_panel(|p| p.brush_size_slider(ui));
        }
    }

    /// The card's fill layers (2026-10-01): one row each — 👁 shows or hides
    /// it (a hidden layer is kept and saved), its name, ✕ removes it from the
    /// card — and its opacity under it. Every change is a recipe edit like a
    /// slider's, so Ctrl+Z covers it; the file in `./out` is never deleted,
    /// so an undone removal has its pixels to come back to.
    fn layer_rows(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        if self.editor.doc().recipe.pixel_layers.is_empty() {
            return;
        }
        sub_caption(ui, tr(lang, K_RETOUCH_LAYERS_ON_THIS_CARD));
        let mut remove = None;
        let mut changed = false;
        for i in 0..self.editor.doc().recipe.pixel_layers.len() {
            ui.horizontal(|ui| {
                let layer = &mut self.editor.doc_mut().recipe.pixel_layers[i];
                if glyph_toggle(ui, layer.enabled, "👁")
                    .on_hover_text(tr(lang, K_RETOUCH_SHOW_OR_HIDE_THIS_LAYER_A_HIDDEN_LAYER_IS_KEPT_A))
                    .clicked()
                {
                    layer.enabled = !layer.enabled;
                    changed = true;
                }
                ui.label(trf(lang, K_RETOUCH_FILL_LAYER, &[("n", &(i + 1).to_string())]))
                    .on_hover_text(layer.path.as_str());
                if glyph(ui, true, "✕")
                    .on_hover_text(tr(lang, K_RETOUCH_REMOVE_THIS_LAYER_FROM_THE_CARD_CTRL_Z_BRINGS_IT))
                    .clicked()
                {
                    remove = Some(i);
                }
            });
            changed |= slider_pct_hinted(
                ui,
                lang,
                tr(lang, K_RETOUCH_LAYER_OPACITY),
                &mut self.editor.doc_mut().recipe.pixel_layers[i].opacity,
                1.0,
                1.0,
                tr(lang, K_RETOUCH_HOW_STRONGLY_THIS_LAYER_COVERS_THE_DEVELOP_UNDER),
            );
        }
        if let Some(i) = remove {
            self.editor.doc_mut().recipe.pixel_layers.remove(i);
            changed = true;
        }
        if changed {
            self.editor.session_mut().dirty = true;
        }
    }

    pub(crate) fn retouch_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang; // Copy — never borrows self, safe inside egui closures.
        ui.add_space(SPACE_LG);
        panel_heading(ui, tr(lang, K_RETOUCH_RETOUCH), false, |_| {});

        // Whole-image Reimagine + reverse-fit MOVED to the AI panel (R22 #4,
        // panels/ai.rs): they are AI develop verbs, and living here put them
        // between the brush tools — a generative re-render of the whole frame
        // sitting in the middle of dust-spot healing. Everything below is
        // PIXEL work on the current rendition.
        //
        // v1.5.0 F9 — what arrived WITH the photograph, above every tool the
        // photographer might reach for, because it is not a tool at all: it is
        // a statement about what is already on this canvas. Its own group, per
        // the user's 2026-09-12 directive that a control belongs to its own
        // section rather than borrowing a neighbour's.
        //
        // Silent on the photographs that carry none, which is 150 of the
        // reference library's 175.
        if !self.editor.doc().recipe.retouch.is_empty() {
            let total = self.editor.doc().recipe.retouch.len();
            let synth =
                self.editor.doc().recipe.retouch.iter().filter(|a| a.origin.is_synthesised()).count();
            sub_caption(ui, tr(lang, K_RETOUCH_IMPORTED_REMOVAL));
            ui.label(
                egui::RichText::new(trf(
                    lang,
                    K_RETOUCH_LIGHTROOM_REMOVED_AREA_S_HERE_RE_SOLVED_FROM_THI,
                    &[("n", &total.to_string())],
                ))
                .weak()
                .small(),
            );
            if synth > 0 {
                // The sentence that keeps the panel honest: for these areas
                // the picture on screen is THIS app's repair, and Adobe's own
                // result is not in the sidecar to be shown.
                ui.label(
                    egui::RichText::new(trf(
                        lang,
                        K_RETOUCH_OF_THEM_WERE_SYNTHESISED_BY_ADOBE_THOSE_PIXELS_A,
                        &[("n", &synth.to_string())],
                    ))
                    .weak()
                    .small(),
                );
                ui.add_enabled_ui(!self.workspace.busy(), |ui| {
                    if action_row(ui, true, tr(lang, K_RETOUCH_REGENERATE_THOSE_AREAS))
                        .on_hover_text(tr(lang, K_RETOUCH_PAINT_THE_SYNTHESISED_AREAS_INTO_GENERATIVE_FILL))
                        .clicked()
                    {
                        // Paint first, and only spawn if something landed —
                        // `start_fill` over a blank mask would refuse with a
                        // status line that reads like the button is broken.
                        if self.editor.paint_imported_removals(&mut self.workspace) > 0 {
                            self.start_fill();
                        }
                    }
                });
            }
        }

        // Every brush lives in the fold whose verb reads it, with a painted
        // area of its own (user decision 2026-09-27; `brush_row`, `BrushOwner`)
        // — the one 「Brush (shared)」 group that hung bare above these three
        // folds, and that the AI panel's Adjust fold read without a control of
        // its own, is gone.
        ui.add_space(SPACE_XS);
        section_fold(ui, "sec_fill", tr(lang, K_RETOUCH_GENERATIVE_FILL), false, true, |ui| {
                // #14a: was `desired_width(f32::INFINITY)`.
                let _field = prompt_field(
                    ui,
                    &mut self.workspace.genai_mut().fill_prompt,
                    tr(lang, K_RETOUCH_WHAT_TO_CHANGE_E_G_REMOVE_THE_WIRES_ON_THE_LEFT),
                );
                #[cfg(test)]
                {
                    self.seams.prompt_rects.push(_field.rect);
                }
                self.brush_row(ui, BrushOwner::Fill);
                // R38: wrapped — quality and the full-res switch share a row
                // that wraps when short; the verb fills the row under them
                // (2026-09-27: a row's one verb takes the whole row).
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("fill_quality")
                        .selected_text(tr(lang, ["high", "medium", "low"][self.workspace.genai().fill_quality.min(2)]))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.workspace.genai_mut().fill_quality, 0, tr(lang, K_AI_HIGH));
                            ui.selectable_value(&mut self.workspace.genai_mut().fill_quality, 1, tr(lang, K_AI_MEDIUM));
                            ui.selectable_value(&mut self.workspace.genai_mut().fill_quality, 2, tr(lang, K_AI_LOW));
                        })
                        .response
                        .on_hover_text(tr(lang, K_AI_GPT_IMAGE_RENDER_QUALITY_HIGHER_LOOKS_BETTER_AND));
                    let src_is_raw = self.editor.active_source_path()
                        .is_some_and(|p| autoshade::decode::is_raw(&p));
                    // Each 「Full-res」 names its verb (R22 #16): fill / heal /
                    // clone / denoise had four identically-labelled checkboxes
                    // across three panels, and this is the only one still gated
                    // RAW-only — a distinction its label could not carry while it
                    // was a twin of the other three.
                    ui.add_enabled(src_is_raw, egui::Checkbox::new(&mut self.workspace.genai_mut().fill_fullres, tr(lang, K_RETOUCH_FULL_RES_FILL)))
                        .on_hover_text(tr(lang, K_RETOUCH_COMPOSITE_ONTO_THE_FULL_SENSOR_DEVELOP_SLOW_RAW));
                });
                // One verb, two ways in (2026-10-12): a painted area is
                // regenerated as before; with none, the words alone go to the
                // model and it chooses where (`start_fill`).
                let painted = self.workspace.area_painted(BrushOwner::Fill);
                let (verb, hint) = if painted {
                    (
                        tr(lang, K_RETOUCH_REMOVE_FILL),
                        tr(lang, K_RETOUCH_REGENERATE_ONLY_THE_PAINTED_AREA_FROM_YOUR_PROMP),
                    )
                } else {
                    (
                        tr(lang, K_RETOUCH_AI_EDIT),
                        tr(lang, K_RETOUCH_SEND_THIS_CARD_S_PICTURE_WITH_YOUR_WORDS_AND_NO),
                    )
                };
                let ready = !self.workspace.busy() && (painted || !self.workspace.genai().fill_prompt.trim().is_empty());
                if primary_row(ui, ready, verb).on_hover_text(hint).clicked() {
                    self.start_fill();
                }
                explainer(ui, lang, tr(lang, K_RETOUCH_WRITE_WHAT_TO_CHANGE_AND_PRESS_AI_EDIT_THE_MODEL));
                self.layer_rows(ui);
            });

        section_fold(ui, "sec_heal", tr(lang, K_RETOUCH_HEAL_PIXEL), false, false, |ui| {
                self.brush_row(ui, BrushOwner::Heal);
                // R38: the two heal verbs share one aligned row and the
                // full-res switch sits under them — three free-width controls
                // in a non-wrapping row overran the default panel width.
                ui.horizontal(|ui| {
                    let cell = columns(ui, 2);
                    ui.add_enabled_ui(!self.workspace.busy(), |ui| {
                        // 🤖, the one AI prefix app-wide (#4): ✦ was this
                        // verb's private glyph. The tooltip carries the
                        // AI-panel cross-reference — this stays HERE, at
                        // the pixels it heals.
                        if action_in(ui, cell, true, tr(lang, K_RETOUCH_AI_HEAL_AUTO))
                            .on_hover_text(ai_xref(lang, tr(lang, K_RETOUCH_A_VISION_MODEL_FINDS_SMALL_DUST_SPOTS_BLEMISHES)))
                            .clicked()
                        {
                            self.start_heal(false, None);
                        }
                        if action_in(ui, cell, true, tr(lang, K_RETOUCH_HEAL_AREA))
                            .on_hover_text(tr(lang, K_RETOUCH_HEAL_THE_BRUSHED_AREA_FROM_SURROUNDING_REAL_PIXE))
                            .clicked()
                        {
                            self.start_heal(true, None);
                        }
                    });
                });
                // Enabled for BAKED sources too (L09#4, user decision
                // 2026-08-11): heal honours the flag on both source
                // types since b4c6c30, but the RAW-only gate left a
                // 61 MP baked TIFF no way to opt out of the 2048px
                // downsample the unchecked path bakes into the master.
                ui.checkbox(&mut self.workspace.genai_mut().heal_fullres, tr(lang, K_RETOUCH_FULL_RES_HEAL))
                    .on_hover_text(tr(lang, K_RETOUCH_HEAL_AT_FULL_RESOLUTION_SLOW_WITHOUT_IT_THE_RETO));
                explainer(ui, lang, tr(lang, K_RETOUCH_AI_AUTO_DETECTS_DUST_BLEMISHES_OR_PAINT_A_MASK_A));
            });

        section_fold(ui, "sec_clone", tr(lang, K_RETOUCH_CLONE_STAMP), false, false, |ui| {
                // The stamp's arm toggle IS its brush toggle (Alt+click samples
                // a source, plain drags paint the Stamp's own area), so the
                // clear verb sits beside it, the radius follows while armed,
                // and the stamp's one verb fills the row under them.
                let armed = self.workspace.brush_armed(BrushOwner::Stamp);
                ui.horizontal(|ui| {
                    let cell = columns(ui, 2);
                    let label = if armed { tr(lang, K_DEVELOP_DONE) } else { tr(lang, K_RETOUCH_ENTER_STAMP) };
                    if action_in(ui, cell, true, label)
                        .on_hover_text(tr(lang, K_RETOUCH_ARM_THE_STAMP_ALT_CLICK_SAMPLES_A_SOURCE_THE_BRU))
                        .clicked()
                    {
                        if armed {
                            self.workspace.disarm_brush();
                        } else {
                            // Arming brings the Stamp's own area onto the
                            // canvas; what was painted for Fill / Heal stays
                            // theirs. Only 「Clear area」 wipes.
                            self.arm_brush(BrushOwner::Stamp);
                            self.workspace.set_status(tr(lang, K_RETOUCH_STAMP_ALT_CLICK_TO_SET_THE_SOURCE_BRUSH_THE_TARG).into());
                        }
                    }
                    if action_in(ui, cell, true, tr(lang, K_RETOUCH_CLEAR_AREA))
                        .on_hover_text(tr(lang, K_RETOUCH_WIPE_THIS_TOOL_S_PAINTED_AREA_THE_OTHER_TOOLS_AR))
                        .clicked()
                    {
                        self.workspace.clear_area(BrushOwner::Stamp);
                    }
                });
                if armed {
                    self.with_panel(|p| p.brush_size_slider(ui));
                }
                if primary_row(ui, !self.workspace.busy() && self.workspace.tools().clone_mode, tr(lang, K_RETOUCH_CLONE_AREA))
                    .on_hover_text(tr(lang, K_RETOUCH_COPY_THE_SAMPLED_SOURCE_OVER_THE_BRUSHED_AREA_VE))
                    .clicked()
                {
                    self.start_clone();
                }
                // Enabled for BAKED sources too — clone_stamp honours
                // the flag on both source types (retouch.rs), the same
                // rule as Heal beside it (L15-7).
                ui.checkbox(&mut self.workspace.tools_mut().clone_fullres, tr(lang, K_RETOUCH_FULL_RES_CLONE))
                    .on_hover_text(tr(lang, K_RETOUCH_CLONE_AT_FULL_RESOLUTION_SLOW_WITHOUT_IT_THE_RET));
                explainer(ui, lang, tr(lang, K_RETOUCH_PHOTOSHOP_STYLE_CLONE_STAMP_ALT_CLICK_TO_SAMPLE));
            });
    }
}
