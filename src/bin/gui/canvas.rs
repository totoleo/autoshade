//! After-image canvas: view transform, overlays, drag handlers.

use super::*;

impl Commands<'_> {







    /// (Re)build the translucent red coverage layer for the active mask. A
    /// coverage key prevents local effect sliders from rebuilding this full-
    /// frame raster: Exposure/Temp/Saturation change WHAT happens inside the
    /// mask, not WHERE it applies. Range masks include their PREFIX
    /// reference recipe (earlier masks applied — the engine's own stacking
    /// rule) because their coverage genuinely depends on pixels.
    pub(crate) fn refresh_mask_overlay(&mut self, ctx: &egui::Context) {
        if !self.workspace.overlay().show_mask_overlay {
            self.workspace.overlay_mut().mask_overlay_tex = None;
            self.workspace.overlay_mut().overlay_key = None;
            return;
        }
        // A handle, not a borrow of `self`: the range reference below asks
        // `self.editor.film_short_edge()`, which caches into `self`.
        let Some(base) = self.editor.photo().base_preview.clone() else {
            self.workspace.overlay_mut().mask_overlay_tex = None;
            self.workspace.overlay_mut().overlay_key = None;
            return;
        };
        // A hovered row previews its coverage; otherwise use the selection.
        let target = self.workspace.overlay().hover_mask
            .filter(|&i| i < self.editor.doc().recipe.masks.len())
            .or_else(|| self.workspace.masks().sel_mask.filter(|&i| i < self.editor.doc().recipe.masks.len()));
        let Some(i) = target else {
            self.workspace.overlay_mut().mask_overlay_tex = None;
            self.workspace.overlay_mut().overlay_key = None;
            return;
        };
        let mask = self.editor.doc().recipe.masks[i].clone();
        // The PREFIX reference with its geometry stripped — ONE builder,
        // shared with the range eyedropper (`range_reference_recipe`, which
        // carries the reasoning). The geometry the coverage WARP still
        // depends on is keyed separately, in OverlayKey below.
        let pre = self.editor.range_reference_recipe(i);
        let lp = &self.editor.doc().recipe.lens_profile;
        let key = OverlayKey {
            base: Arc::as_ptr(&base) as usize,
            target: i,
            mask: mask.mask.clone(),
            components: mask.components.clone(),
            enabled: mask.enabled,
            range: mask.range,
            amount: mask.amount,
            inverted: mask.inverted,
            reference_recipe: mask.range.is_some().then(|| pre.clone()),
            straighten_deg: self.editor.doc().recipe.straighten_deg,
            lens_distortion: self.editor.doc().recipe.lens_distortion,
            profile_dist_on: lp.distortion_on && !lp.distortion.is_empty(),
            profile_ca_on: lp.ca_on && !lp.ca_r.is_empty() && !lp.ca_b.is_empty(),
            manual_ca: (self.editor.doc().recipe.ca_r, self.editor.doc().recipe.ca_b),
        };
        if self.workspace.overlay().overlay_key.as_ref() == Some(&key) && self.workspace.overlay().mask_overlay_tex.is_some() {
            return;
        }

        // A geometry-only mask never reads reference pixels. Avoid the old
        // second develop entirely; only Range Masks need the masks-cleared
        // developed reference and its recipe-keyed cache.
        // KNOWN COST (C21): this is a ONE-slot cache compared by full-recipe
        // equality, and the reference is now the mask PREFIX rather than a
        // masks-cleared develop — so sweeping the pointer down a list of
        // Range masks alternates prefixes and misses the cache on every row,
        // paying a synchronous preview develop each time. Correctness first:
        // the prefix IS what the engine evaluates a range against, and a
        // wrong overlay is worse than a slow one. Keyed multi-slot caching is
        // the fix and is recorded in the roadmap, not smuggled in here.
        let reference: &image::DynamicImage = if mask.range.is_some() {
            if !matches!(&self.workspace.overlay().overlay_ref, Some((r, _)) if *r == pre) {
                // Rebuilding the masks-cleared reference is a full develop on
                // the UI thread — the 100-300 ms class (2560/4096) the async
                // preview path exists to hide. Mid-gesture (global-slider
                // drag), keep showing the previous coverage and re-arm the
                // stale flag; the rebuild lands on the frame after the pointer
                // settles. Geometry-only masks (the else arm) stay instant.
                if ctx.input(|i| i.pointer.any_down()) {
                    self.workspace.overlay_mut().overlay_stale = true;
                    return;
                }
                // No downstream geometry, explicitly: this reference exists to
                // be SAMPLED for pixel values. The profile still matters for
                // LINEAR's handle-only raw-frame rule; RADIAL remains stored.
                // The canvas's own film edge: the Detail passes ran before this
                // mask on the canvas, at that scale, so the range must too.
                let film = self.editor.film_short_edge();
                let raw_source = self.editor.source_is_raw();
                let composed = autoshade::render::geometry_profile(&self.editor.doc().recipe);
                let img = autoshade::render::develop_preview_framed(
                    &base,
                    &pre,
                    &autoshade::diag::pixels(),
                    self.editor.sampled_frame(&composed),
                    film,
                    raw_source,
                );
                self.workspace.overlay_mut().overlay_ref = Some((pre, img));
            }
            &self.workspace.overlay().overlay_ref.as_ref().expect("range reference cached").1
        } else {
            base.as_ref()
        };
        // Coverage is a display-only translucent layer (LINEAR-filtered when
        // painted), so it never needs more than ~1k resolution — at 2560/4096
        // preview edges the full-frame raster + RGBA pass dominated the
        // rebuild. Range weights are judged on the downscaled reference: the
        // same pixels box-filtered, indistinguishable for an overlay.
        let small;
        let cov_ref: &image::DynamicImage = if reference.width().max(reference.height()) > 1024 {
            small = autoshade::render::thumbnail(reference, 1024, 1024);
            &small
        } else {
            reference
        };
        // The COMPOSED profile (R25 B3) — manual CA rides the same knots.
        // Hoisted above the coverage build so ONE `MaskFrame` decides both the
        // overlay's own frame adaptation and whether it is warped below — the
        // same rule the render arms follow (R29 Batch-3). A parametric mask is
        // stored in Lightroom's post-correction frame and the render now puts
        // it back there; an overlay that skipped the adaptation would advertise
        // coverage the render does not apply, by the whole field.
        // On a corrected base (v1.17.0) the coverage is evaluated in that frame
        // and needs no warp, as the canvas's own pixels need none.
        let cov_geom = autoshade::render::geometry_profile(&self.editor.doc().recipe);
        let cov_frame =
            autoshade::render::MaskFrame::of_base(&cov_geom, self.editor.doc().recipe.lens_distortion, self.editor.base_corrected());
        let mut cov = image::DynamicImage::ImageLuma8(autoshade::render::mask_coverage(
            &mask, cov_ref, cov_frame,
        ));
        if cov_frame.warps() {
            // The coverage overlay must follow the SAME geometric chain as the
            // rendered pixels — profile distortion included, or the red wash
            // drifts off its mask near the frame edges.
            cov = autoshade::render::apply_lens_geometry(
                &cov,
                &cov_geom,
                self.editor.doc().recipe.lens_distortion,
            );
        }
        if self.editor.doc().recipe.straighten_deg != 0.0 {
            cov = autoshade::render::rotate_straighten(&cov, self.editor.doc().recipe.straighten_deg);
        }
        let g = cov.to_luma8();
        let (w, h) = (g.width() as usize, g.height() as usize);
        let mut rgba = vec![0u8; w * h * 4];
        for (i, p) in g.pixels().enumerate() {
            rgba[i * 4..i * 4 + 4]
                .copy_from_slice(&[255, 40, 40, (p[0] as u16 * 140 / 255) as u8]);
        }
        let colour = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
        if let Some(tex) = &mut self.workspace.overlay_mut().mask_overlay_tex {
            tex.set(colour, egui::TextureOptions::LINEAR);
        } else {
            self.workspace.overlay_mut().mask_overlay_tex =
                Some(ctx.load_texture("mask_overlay", colour, egui::TextureOptions::LINEAR));
        }
        self.workspace.overlay_mut().overlay_key = Some(key);
        self.workspace.overlay_mut().overlay_build_count += 1;
    }




    pub(crate) fn after_view(&mut self, ui: &mut egui::Ui, max_w: f32, avail_y: f32, comparing: bool) {
        let lang = self.lang;
        let tex = if comparing { self.editor.plates().before_tex.as_ref() } else { self.editor.plates().after_tex.as_ref() };
        let Some((id, tex_size)) = tex.map(|t| (t.id(), t.size_vec2())) else {
            // First develop still in flight: say so — a bare "…" read as a
            // hang, a spinner reads as work.
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(Text::new(tr(lang, K_CANVAS_PREPARING_PREVIEW)).weak());
            });
            return;
        };

        let uv = self.editor.view_uv(&mut self.workspace);
        // Display size fits the VISIBLE window's aspect (in image pixels).
        let vis_px = egui::vec2(uv.width() * tex_size.x, uv.height() * tex_size.y);
        // Past fit the 4× upscale cap is DROPPED (L13-2): the cap tamed tiny
        // images at fit, but the visible window shrinks as zoom grows, so at
        // deep zoom the cap re-fit the canvas SMALLER — zooming in shrank
        // the picture and broke the cursor anchor. A zoomed view fills the
        // pane box.
        let disp = fit_in_capped(
            vis_px,
            max_w,
            avail_y,
            if self.workspace.view().zoom > 1.0 { f32::INFINITY } else { 4.0 },
        );
        // PHYSICAL pixels per image pixel: disp is measured in egui logical
        // points — without pixels_per_point a "100%" readout on a 2× display
        // spanned two physical pixels per texel, and "1:1" wasn't.
        let ppp = ui.ctx().pixels_per_point();
        let scale = disp.x * ppp / vis_px.x.max(1.0); // display (physical) px per image px
        // The zoom that makes `scale` exactly 1.0, stored for the `1` key
        // which runs at frame start, before vis_px/disp exist; the 1:1
        // button and double-click read it too. Solved at FIT geometry:
        // w_tex (full crop in texels) is zoom-invariant, but the naive
        // vis/disp form was not — fit_in's 4× upscale cap makes disp track
        // vis on small images, so that form varied with live zoom (Codex
        // 阶段5 F2). Pane-bound displays give the identical value; a photo
        // already coarser than 1:1 at fit clamps to 1 (fit is closest).
        let w_tex = vis_px * self.workspace.view().zoom;
        let disp_fit = fit_in(w_tex, max_w, avail_y);
        self.workspace.view_mut().one_to_one = (w_tex.x / (disp_fit.x * ppp)).clamp(1.0, 12.0);

        // Caption row: mode hint left, zoom readout + Fit / 1:1 right. Every
        // armed-tool hint names its exit (Esc), and the placement hint speaks
        // the armed KIND's language instead of calling a radial a gradient.
        let hint = if comparing {
            if self.workspace.view().before {
                // The latch came from \ — "release B" was a lie there (M21).
                tr(lang, K_CANVAS_BEFORE_SOURCE_PRESS_AGAIN_OR_ESC_TO_RETURN_TO_ED)
            } else {
                tr(lang, K_CANVAS_BEFORE_SOURCE_RELEASE_B_TO_RETURN_TO_EDITING)
            }
        } else if self.workspace.crop().mode {
            tr(lang, K_CANVAS_CROP_DRAG_CORNERS_EDGES_TO_RESIZE_INSIDE_TO_MOVE)
        } else if let Some((kind, _)) = self.workspace.masks().placing_mask {
            match kind {
                MaskKind::Linear => tr(lang, K_CANVAS_LINEAR_GRADIENT_DRAG_FROM_THE_FULLY_APPLIED_SIDE),
                MaskKind::Radial => tr(lang, K_CANVAS_RADIAL_GRADIENT_DRAG_TO_DRAW_AN_ELLIPTICAL_AREA),
            }
        } else if self.workspace.tools().wb_picking {
            tr(lang, K_CANVAS_WB_EYEDROPPER_CLICK_A_SPOT_THAT_SHOULD_BE_NEUTRA)
        } else if self.workspace.tools().range_picking.is_some() {
            tr(lang, K_CANVAS_COLOR_RANGE_CLICK_THE_COLOR_TO_PICK_IN_THE_IMAGE)
        } else if self.workspace.tools().point_color_picking {
            tr(lang, K_CANVAS_POINT_COLOR_EYEDROPPER_CLICK_THE_COLOUR_TO_ADJUS)
        } else if self.workspace.tools().clone_mode {
            tr(lang, K_CANVAS_STAMP_ALT_CLICK_TO_SET_THE_SOURCE_DRAG_TO_BRUSH)
        } else if self.workspace.masks().paint_mode {
            if self.workspace.masks().mask_brush.is_some() {
                tr(lang, K_CANVAS_MASK_BRUSH_PAINT_TO_SELECT_ERASE_REMOVES_APPLY_B)
            } else {
                match self.workspace.masks().paint_owner {
                    BrushOwner::Fill => tr(lang, K_CANVAS_BRUSH_PAINT_THE_AREA_TO_REMOVE_OR_FILL_ESC_TO_EX),
                    BrushOwner::Heal => tr(lang, K_CANVAS_BRUSH_PAINT_THE_AREA_TO_HEAL_ESC_TO_EXIT),
                    BrushOwner::Stamp => tr(lang, K_CANVAS_BRUSH_PAINT_THE_AREA_TO_CLONE_OVER_ESC_TO_EXIT),
                    BrushOwner::Adjust => tr(lang, K_CANVAS_BRUSH_PAINT_THE_AREA_TO_ADJUST_ESC_TO_EXIT),
                }
            }
        } else {
            tr(lang, K_CANVAS_AFTER_DRAG_A_BOX_LOCAL_AI_SCROLL_TO_ZOOM_SPACE_M)
        };
        // An armed tool must be visible at NORMAL contrast — .weak().small()
        // is the passive-help style, and it was the only always-on indicator
        // when the arming control sat in a collapsed section.
        let armed = !comparing && self.workspace.tool_armed();
        ui.horizontal(|ui| {
            if armed {
                ui.label(Text::new(hint).color(self.workspace.ui().theme.colors().armed_hint).small());
            } else {
                ui.label(Text::new(hint).weak().small());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if action(ui, true, "1:1").on_hover_text(tr(lang, K_CANVAS_PREVIEW_PIXELS_1_1_DOUBLE_CLICK_THE_IMAGE_TO_TOG)).clicked() {
                    // Same ceiling as the render path (view_uv clamps at 12) —
                    // an unclamped value desynced zoom/pan math from the view.
                    // ppp: 1:1 means one texel per PHYSICAL pixel — the value
                    // computed above, shared with the `1` key. Target only:
                    // the update() glide carries `zoom` there.
                    self.workspace.view_mut().zoom_target = self.workspace.view().one_to_one;
                }
                // "Fit" is natural language, unlike its "1:1" sibling — it
                // must route through `tr` like every user-facing literal
                // (the i18n module contract; the audit now flags bypasses).
                if action(ui, true, tr(lang, K_CANVAS_FIT)).on_hover_text(tr(lang, K_CANVAS_FIT_THE_WHOLE_IMAGE_TO_THE_CANVAS_DOUBLE_CLICK_T)).clicked() {
                    self.workspace.view_mut().zoom_target = 1.0; // glides; the pan clamp eases the rest
                    self.workspace.view_mut().pan = egui::vec2(0.5, 0.5);
                }
                if glyph_toggle(ui, self.workspace.chrome().show_clipping, "▲")
                    .on_hover_text(tr(lang, K_CANVAS_CLIPPING_WARNING_J_RED_HIGHLIGHT_CLIP_BLUE_SHADO))
                    .clicked()
                {
                    let ctx = ui.ctx().clone();
                    self.with_panel(|p| p.toggle_clipping(&ctx)); // instant — no redevelop
                }
                ui.label(Text::new(format!("{:.0}%", scale * 100.0)).weak().small());
                // --- preview resolution (gap batch E): 1:1 that actually
                // resolves detail. Switching re-decodes the current photo with
                // the recipe KEPT (the keep_recipe path from batch B).
                let before = self.workspace.genai().preview_edge;
                ui.add_enabled_ui(!self.workspace.busy(), |ui| {
                    ui.menu("preview_edge", format!("{}px", self.workspace.genai().preview_edge), Some(64.0), |ui| {
                            ui.selectable_value(&mut self.workspace.genai_mut().preview_edge, 1280, tr(lang, K_CANVAS_1280PX_FLUID));
                            ui.selectable_value(&mut self.workspace.genai_mut().preview_edge, 2560, "2560px");
                            ui.selectable_value(&mut self.workspace.genai_mut().preview_edge, 4096, tr(lang, K_CANVAS_4096PX_INSPECT));
                        })
                        .on_hover_text(tr(lang, K_CANVAS_WORKING_PREVIEW_RESOLUTION_1280_IS_SMOOTHEST_ON));
                });
                if self.workspace.genai().preview_edge != before
                    && let Some(p) = self.editor.photo().src_path.clone()
                {
                    if self.workspace.busy() {
                        // An already-open popup outlives the disabled combo
                        // (its own egui Area), so this is reachable while
                        // busy — and the combo has ALREADY mutated
                        // preview_edge. Keeping the new value displayed a
                        // resolution the working preview does not have,
                        // unreachable to boot (the != trigger above is a
                        // one-shot), persisted it into Prefs, and fed it to
                        // every bake site. The switch reverts with the
                        // refusal, and the status says so instead of
                        // dropping it silently.
                        self.workspace.genai_mut().preview_edge = before;
                        // BY TOAST (the switch_variant / Ctrl+S refusal
                        // rule): this runs during the draw phase, and a
                        // status line dies under the very next worker
                        // message — one frame, then the combo just snaps
                        // back with no visible reason.
                        let t = tr(
                            lang,
                            K_CANVAS_BUSY_THE_PREVIEW_RESOLUTION_SWITCH_WAS_NOT_APPLI,
                        )
                        .to_string();
                        self.workspace.toast(ToastKind::Error, t);
                    } else {
                        // The pre-switch edge rides the flight: a FAILED
                        // keep-flight leaves the canvas at the OLD edge, and
                        // preview_edge must not keep displaying (and feeding
                        // every bake site) a resolution the preview never
                        // reached — the same residue the busy refusal above
                        // is cured of.
                        self.workspace.open_mut().edge_before_flight = Some(before);
                        self.workspace.cards_mut().keep_recipe = true; // re-decode, keep the edit
                        self.open_path(p);
                    }
                }
            });
        });

        let (rect, resp) = ui.allocate_exact_size(disp, egui::Sense::click_and_drag());
        self.editor.paint_canvas_image(ui, rect, uv, id, comparing);
        // Diagnostic layers, uv-synced with the image (hidden while comparing):
        // clipping warnings, then the selected mask's coverage on top.
        if !comparing {
            if let Some(t) = &self.workspace.chrome().clip_tex {
                ui.painter_at(rect).image(t.id(), rect, uv, egui::Color32::WHITE);
            }
            if let Some(t) = &self.workspace.overlay().mask_overlay_tex {
                ui.painter_at(rect).image(t.id(), rect, uv, egui::Color32::WHITE);
            }
        }
        let xf = ViewXform { rect, uv };

        // --- zoom to cursor (scroll) -----------------------------------------
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.1
                && let Some(p) = resp.hover_pos()
            {
                let half = 0.5 / self.workspace.view().zoom;
                let (fx, fy) = (
                    ((p.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0),
                    ((p.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0),
                );
                // Cursor's point in crop-window coords, kept stationary across the zoom.
                let q = egui::vec2(
                    self.workspace.view().pan.x - half + fx * 2.0 * half,
                    self.workspace.view().pan.y - half + fy * 2.0 * half,
                );
                // Scroll stays INSTANT (it is already incremental, and the
                // cursor-anchored pan re-solve needs the immediate zoom) —
                // the target follows so no stale glide fights the wheel.
                self.workspace.view_mut().zoom = (self.workspace.view().zoom * (scroll * 0.003).exp()).clamp(1.0, 12.0);
                self.workspace.view_mut().zoom_target = self.workspace.view().zoom;
                let nh = 0.5 / self.workspace.view().zoom;
                self.workspace.view_mut().pan = q - egui::vec2((fx - 0.5) * 2.0 * nh, (fy - 0.5) * 2.0 * nh);
            }
        }
        let tool_active = self.workspace.tool_armed();
        // Double-click toggles fit ↔ 1:1 (preview pixels) — but never while a
        // canvas tool is armed: a quick second tap inside brush/crop/pick used
        // to teleport the view instead of reaching the tool. Targets only —
        // the update() glide replaces the old teleport with a ~120 ms ease.
        if resp.double_clicked() && !tool_active {
            if self.workspace.view().zoom_target > 1.01 {
                self.workspace.view_mut().zoom_target = 1.0;
                self.workspace.view_mut().pan = egui::vec2(0.5, 0.5);
            } else {
                // Same physical-pixel 1:1 target as the button above — the
                // zoom-invariant form computed at the top of this fn (the
                // old `vis_px.x / (disp.x*ppp)` was only right at zoom≈1,
                // which the pre-glide branch guard used to guarantee).
                self.workspace.view_mut().zoom_target = self.workspace.view().one_to_one;
            }
            // The pair's FIRST release cleared the AI region through the
            // plain-click path — a double-click means "toggle zoom", so put
            // it back. Bounded window: a much later double-click elsewhere
            // must not resurrect a long-cleared region (M18).
            if let Some((r, t)) = self.workspace.overlay_mut().region_restore.take()
                && t.elapsed() < Duration::from_millis(600)
            {
                self.workspace.overlay_mut().region = Some(r);
            }
        }

        // --- pan: middle-drag, Space + left-drag, or (the LR gesture) a plain
        // left-drag while ZOOMED IN — a zoomed-in drag means "pan" in every
        // photo editor, and requiring Space for it was a top "feels off".
        // Box-select stays reachable while zoomed via Ctrl+drag; an active
        // tool, a mask-knob hover/drag or a box drag in flight all take
        // priority over the implicit pan.
        // Focus-gated like B-compare: Space in a text field is a space.
        let space = ui.ctx().memory(|m| m.focused()).is_none()
            && ui.input(|i| i.key_down(egui::Key::Space));
        let ctrl = ui.input(|i| i.modifiers.command);
        let over_knob = self.workspace.masks().mask_drag.is_some()
            || self.editor.sel_target_geometry(&self.workspace).is_some_and(|g| {
                let (dims, deg, dist) = self.editor.geom_ctx();
                // Probe BOTH positions: egui reports drag_started only
                // after ~6 px of travel, so a fast flick's CURRENT pointer
                // has already left the knob while its press origin is still
                // on it — `or_else` (origin only when hover was None) still
                // let pan steal an in-canvas flick. The knobs probed are the
                // ones actually shown: the selected COMPONENT's when one is
                // selected (sel_target_geometry), matching handle_mask_edit.
                let handles = mask_handle_points(&geom_to_view(g, dims, deg, &dist), xf);
                let hits = |p: egui::Pos2| handles.iter().any(|(_, hp)| hp.distance(p) <= HANDLE_HIT);
                resp.hover_pos().is_some_and(hits)
                    || ui.input(|i| i.pointer.press_origin()).is_some_and(hits)
            });
        let zoom_pan = self.workspace.view().zoom > 1.001
            && !tool_active
            && !ctrl
            && !over_knob
            && self.workspace.overlay().region_drag.is_none();
        let panning = resp.dragged_by(egui::PointerButton::Middle)
            || ((space || zoom_pan) && resp.dragged_by(egui::PointerButton::Primary));
        if panning {
            let d = resp.drag_delta();
            let ext = 1.0 / self.workspace.view().zoom; // visible extent in crop-window coords
            self.workspace.view_mut().pan -= egui::vec2(
                d.x / rect.width().max(1.0) * ext,
                d.y / rect.height().max(1.0) * ext,
            );
        }

        // Cursor language: say what a click/drag would do right now. The pick
        // tools (WB / range / point colour / clone-source) set their own
        // crosshair in their handlers; this covers the hand for panning and the
        // drawing tools.
        if panning {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if (space || zoom_pan) && resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        } else if resp.hovered()
            && (self.workspace.masks().paint_mode || self.workspace.masks().placing_mask.is_some() || self.workspace.crop().mode)
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }

        if comparing || panning {
            return; // tools pause while comparing / panning
        }

        // --- tool dispatch (one active interaction at a time) -----------------
        if self.workspace.crop().mode {
            self.handle_crop(ui, &resp, xf, tex_size);
        } else if self.workspace.masks().placing_mask.is_some() {
            self.handle_place_mask(ui, &resp, xf);
        } else if self.workspace.tools().wb_picking {
            self.handle_wb_pick(ui, &resp, xf);
        } else if self.workspace.tools().range_picking.is_some() {
            self.handle_range_pick(ui, &resp, xf);
        } else if self.workspace.tools().point_color_picking {
            self.handle_point_color_pick(ui, &resp, xf);
        } else if self.workspace.tools().lens_focus_picking {
            self.editor.draw_lens_focus(ui, xf);
            if let Some(action) = self.editor.handle_lens_focus_pick(ui, &resp, xf) {
                self.apply_actions(vec![action]);
            }
        } else if self.workspace.tools().clone_mode {
            self.handle_clone(ui, &resp, xf);
            self.editor.ensure_mask_tex(&mut self.workspace, ui.ctx());
            if let Some(t) = &self.workspace.overlay().mask_tex {
                ui.painter_at(rect).image(t.id(), rect, uv, egui::Color32::WHITE);
            }
            // Alt is the source pick, with its own crosshair.
            if !ui.input(|i| i.modifiers.alt) {
                self.draw_brush_cursor(ui, &resp, rect);
            }
        } else if self.workspace.masks().paint_mode {
            self.handle_paint(&resp, xf);
            self.editor.ensure_mask_tex(&mut self.workspace, ui.ctx());
            if let Some(t) = &self.workspace.overlay().mask_tex {
                ui.painter_at(rect).image(t.id(), rect, uv, egui::Color32::WHITE);
            }
            self.draw_brush_cursor(ui, &resp, rect);
        } else {
            // The selected mask's on-image knobs take priority over box-select
            // (a knob hit means "edit the mask", never "start a region").
            if !self.handle_mask_edit(ui, &resp, xf) {
                self.handle_region_select(ui, &resp, xf);
            }
        }

        // The committed AI region (and a drag in flight) paints EVERY frame it
        // exists — it still feeds the analyze prompt while a tool or a knob
        // hover owns the pointer, so hiding it there misrepresented what the
        // AI would see.
        {
            let stroke = egui::Stroke::new(2.0, ACCENT);
            let fill =
                egui::Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 40);
            let draw = |r: egui::Rect| {
                ui.painter().rect_filled(r, 0.0, fill);
                ui.painter().rect_stroke(r, 0.0, stroke, egui::StrokeKind::Middle);
            };
            if let Some((s, e)) = self.workspace.overlay().region_drag {
                draw(egui::Rect::from_two_pos(s, e).intersect(rect));
            } else if let Some([l, t, rr, bb]) = self.workspace.overlay().region {
                // Stored in the original frame → back into the transformed view.
                let ((bw, bh), deg, dist) = self.editor.geom_ctx();
                let a = orig_norm_to_view(l, t, (bw, bh), deg, &dist);
                let b2 = orig_norm_to_view(rr, bb, (bw, bh), deg, &dist);
                // from_two_pos, like the drag path above (L13-6): under a
                // straighten the transformed corners can swap order, and
                // from_min_max built a negative rect that was culled — the
                // committed region turned invisible while still feeding AI.
                draw(
                    egui::Rect::from_two_pos(xf.to_screen(a.0, a.1), xf.to_screen(b2.0, b2.1))
                        .intersect(rect),
                );
            }
        }

        // Selected mask stays visualised so its sliders have visual feedback
        // (geometry is stored in the original frame → map into the view),
        // with the editing knobs on top (drag = reshape/move, handle_mask_edit).
        // When one of the mask's COMPONENTS is selected, the outline and
        // knobs target THAT shape — the same geometry the edit handler
        // mutates (sel_target_geometry).
        if !self.workspace.crop().mode
            && self.workspace.masks().placing_mask.is_none()
            && let Some(m) = self.workspace.masks().sel_mask.and_then(|i| self.editor.doc().recipe.masks.get(i))
        {
            let target = match self.workspace.masks().sel_component {
                Some(c) if c < m.components.len() => &m.components[c].geometry,
                _ => &m.mask,
            };
            let (dims, deg, dist) = self.editor.geom_ctx();
            let vg = geom_to_view(target, dims, deg, &dist);
            // THE shared predicate (Codex AL F1): the true outline must
            // warp whenever the pixels' green map does.
            let geom_active =
                autoshade::render::geometry_moves_frame(&dist.profile, dist.amount);
            // `LocalAdjustment::net_inverted` against the geometry actually on
            // screen: `target` may be a COMPONENT, whose own bit is the one
            // that composes with the correction's flag here — so the pair is
            // spelled once, through `MaskGeometry::own_inverted`, rather than
            // re-XORing a radial's `flipped` at each marker.
            let net_inverted = m.inverted != target.own_inverted();
            if let MaskGeometry::Radial { top, left, bottom, right, angle, .. } = target
                && (deg != 0.0 || geom_active)
            {
                // The engine evaluates the ellipse in the ORIGINAL frame; under
                // straighten/distortion its image is a rotated/warped curve —
                // not the axis-aligned ellipse the bbox mapping yields. Sample
                // it parametrically — the mask's OWN rotation folded in first,
                // the same order the engine applies — and draw the true
                // outline. (Knobs keep the bbox positions; HANDLE_HIT absorbs
                // the difference.)
                let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
                let (rx, ry) = ((right - left) / 2.0, (bottom - top) / 2.0);
                let (s_a, c_a) = angle.to_radians().sin_cos();
                let pts: Vec<egui::Pos2> = (0..48)
                    .map(|k| {
                        let th = k as f32 / 48.0 * std::f32::consts::TAU;
                        let (ex, ey) = (rx * th.cos(), ry * th.sin());
                        let (vx, vy) = orig_norm_to_view(
                            cx + ex * c_a - ey * s_a,
                            cy + ex * s_a + ey * c_a,
                            dims,
                            deg,
                            &dist,
                        );
                        xf.to_screen(vx, vy)
                    })
                    .collect();
                let p = ui.painter_at(xf.rect);
                p.add(egui::Shape::closed_line(pts, egui::Stroke::new(2.0, ACCENT)));
                // Same effect-side marker semantics as draw_mask_overlay.
                let (ccx, ccy) = orig_norm_to_view(cx, cy, dims, deg, &dist);
                let cs = xf.to_screen(ccx, ccy);
                if net_inverted {
                    p.circle_stroke(cs, 3.0, egui::Stroke::new(2.0, ACCENT));
                } else {
                    p.circle_filled(cs, 3.0, ACCENT);
                }
            } else {
                draw_mask_overlay(ui, xf, &vg, net_inverted, self.lang);
            }
            let p = ui.painter_at(xf.rect);
            for (h, pos) in mask_handle_points(&vg, xf) {
                let r = if h == 0 { 5.5 } else { 4.5 }; // centre knob reads bigger
                p.circle_filled(pos, r, egui::Color32::WHITE);
                p.circle_stroke(pos, r, egui::Stroke::new(1.5, ACCENT));
            }
        }
    }

    /// WB eyedropper: click a pixel that SHOULD be neutral and the engine's
    /// inverse solver (`render::solve_wb_for` — the same forward model the
    /// render applies to this recipe, anchored at this photo's stamped
    /// as-shot white, or the legacy 5500 K) turns it into Temp + Tint.
    /// Samples a 5×5 mean of the BASE plate (`base_preview` — the active
    /// card's own pixel source: the source neutral, or a baked master): WB
    /// runs before develop, so the solve must see pre-develop pixels, not
    /// the current edit.
    pub(crate) fn handle_wb_pick(&mut self, ui: &egui::Ui, resp: &egui::Response, xf: ViewXform) {
        let Some(orig) = self.editor.clicked_orig_norm(ui, resp, xf) else { return };
        let (nx, ny) = self.editor.orig_to_base_norm(orig);
        let px = {
            let Some(base) = &self.editor.photo().base_preview else { return };
            let Some(px) = sample_5x5_mean(base, nx, ny) else { return };
            px
        };
        self.apply_actions(vec![logic::Action::Pick(logic::pick::PickAction::WhiteBalance(px))]);
    }

    /// Colour-range sample: click keys the pending mask's Color range to that
    /// spot. Samples a 5×5 mean of the PREFIX develop (this recipe cut down
    /// to the masks before this one, geometry stripped —
    /// `range_reference_recipe`): the exact pixel state `apply_masks`
    /// evaluates range weights against, so the picked colour is what the
    /// engine will match. The reference is the coverage overlay's own,
    /// shared through `overlay_ref`: with the overlay on, a click is a 5×5
    /// read; a miss pays one preview-sized develop ≈ one slider tick.
    pub(crate) fn handle_range_pick(&mut self, ui: &egui::Ui, resp: &egui::Response, xf: ViewXform) {
        let Some((nx, ny)) = self.editor.clicked_orig_norm(ui, resp, xf) else { return };
        let Some(mi) = self.workspace.tools().range_picking.filter(|&i| i < self.editor.doc().recipe.masks.len()) else {
            self.workspace.tools_mut().range_picking = None; // stale index (mask deleted mid-pick)
            return;
        };
        let smp = {
            let Some(base) = self.editor.photo().base_preview.clone() else { return };
            // The PREFIX reference (masks before this one — the engine
            // evaluates a range on the pixel as it stands when THIS mask
            // runs), built by the coverage overlay's OWN builder
            // (`range_reference_recipe` — one door, so the two cannot drift
            // again) and SHARING its cache: with the overlay on, a click
            // costs a 5×5 read instead of a full preview develop on the UI
            // thread — and a miss here pre-warms the overlay's next rebuild.
            // A masks-CLEARED reference judged mask 2's range on pixels mask
            // 0/1 had already moved (CX5-6).
            let pre = self.editor.range_reference_recipe(mi);
            if !matches!(&self.workspace.overlay().overlay_ref, Some((r, _)) if *r == pre) {
                // The explicit no-downstream frame and film edge for the same
                // reasons as the overlay build above, and they must MATCH it —
                // they share cache.
                let film = self.editor.film_short_edge();
                let raw_source = self.editor.source_is_raw();
                let composed = autoshade::render::geometry_profile(&self.editor.doc().recipe);
                let img = autoshade::render::develop_preview_framed(
                    &base,
                    &pre,
                    &autoshade::diag::pixels(),
                    self.editor.sampled_frame(&composed),
                    film,
                    raw_source,
                );
                self.workspace.overlay_mut().overlay_ref = Some((pre, img));
            }
            let (bx, by) = self.editor.orig_to_base_norm((nx, ny));
            let reference = &self.workspace.overlay().overlay_ref.as_ref().expect("range reference cached").1;
            let Some(smp) = sample_5x5_mean(reference, bx, by) else { return };
            smp
        };
        self.apply_actions(vec![logic::Action::Pick(logic::pick::PickAction::RangeColor {
            mask: mi,
            rgb: smp,
            px: nx,
            py: ny,
        })]);
    }

    /// Point Color eyedropper (v1.5.0): click a colour and a swatch keyed to
    /// it joins the Color Mixer section.
    ///
    /// Samples a 5×5 mean of the develop as it stands WHERE THE POINT COLOURS
    /// RUN (`render::point_color_sampling_recipe`: the grade, the finishing
    /// passes, the masks and the swatches themselves switched off), so the
    /// swatch matches the pixel `render::apply_point_colors` will test rather
    /// than the finished look — the Range mask's rule, for the same reason,
    /// and at the same price: one preview-sized develop per click. The frame
    /// and the film edge are that build's too (no geometry stage, the canvas's
    /// own film edge), and the click maps back to the original frame first.
    pub(crate) fn handle_point_color_pick(&mut self, ui: &egui::Ui, resp: &egui::Response, xf: ViewXform) {
        let Some((nx, ny)) = self.editor.clicked_orig_norm(ui, resp, xf) else { return };
        let px = {
            let Some(base) = self.editor.photo().base_preview.clone() else { return };
            let reference = autoshade::render::point_color_sampling_recipe(&self.editor.doc().recipe);
            let film = self.editor.film_short_edge();
            let raw_source = self.editor.source_is_raw();
            let composed = autoshade::render::geometry_profile(&self.editor.doc().recipe);
            let img = autoshade::render::develop_preview_framed(
                &base,
                &reference,
                &autoshade::diag::pixels(),
                self.editor.sampled_frame(&composed),
                film,
                raw_source,
            );
            let (bx, by) = self.editor.orig_to_base_norm((nx, ny));
            let Some(px) = sample_5x5_mean(&img, bx, by) else { return };
            px
        };
        self.apply_actions(vec![logic::Action::Pick(logic::pick::PickAction::PointColor(px))]);
    }


    /// On-image editing of the SELECTED mask's geometry (the LR gesture):
    /// drag an end/edge knob to reshape, the centre knob to move the whole
    /// mask — no more redraw-from-scratch via 重画. Geometry lives in the
    /// ORIGINAL frame, so every write maps the pointer back through
    /// view_norm_to_orig — the same chain placement uses. Returns true while
    /// it owns the pointer (hovering a knob or mid-drag) so box-select
    /// doesn't also react; a box-select drag already in flight keeps
    /// priority (else its live rectangle would freeze mid-air whenever the
    /// pointer crossed a knob).
    pub(crate) fn handle_mask_edit(&mut self, ui: &egui::Ui, resp: &egui::Response, xf: ViewXform) -> bool {
        if self.workspace.overlay().region_drag.is_some() {
            return false;
        }
        let Some(target_geom) = self.editor.sel_target_geometry(&self.workspace).cloned() else {
            self.workspace.masks_mut().mask_drag = None;
            return false;
        };
        let (dims, deg, dist) = self.editor.geom_ctx();
        let view_geom = geom_to_view(&target_geom, dims, deg, &dist);
        let handles = mask_handle_points(&view_geom, xf);
        if handles.is_empty() {
            self.workspace.masks_mut().mask_drag = None; // bitmap: nothing parametric to drag
            return false;
        }
        // A stranded grab (Esc / hold-B / pan swallowed the drag_stopped frame)
        // must not turn the NEXT unrelated drag into a mask reshape: no drag in
        // flight ⇒ no grab.
        if !resp.dragged() && !resp.drag_started() {
            self.workspace.masks_mut().mask_drag = None;
        }
        // Nearest handle wins, ties broken toward the edge/end knobs — the
        // centre-move knob is emitted first and used to shadow them whenever a
        // small mask put everything within HANDLE_HIT.
        let pick = |p: egui::Pos2| {
            handles
                .iter()
                .filter(|(_, hp)| hp.distance(p) <= HANDLE_HIT)
                .min_by(|a, b| {
                    a.1.distance(p).total_cmp(&b.1.distance(p)).then(b.0.cmp(&a.0))
                })
                .map(|(h, _)| *h)
        };
        let hover_h = resp.hover_pos().and_then(pick);
        let orig_at = |p: egui::Pos2| {
            let (nx, ny) = xf.to_norm(p);
            view_norm_to_orig(nx, ny, dims, deg, &dist)
        };
        if resp.drag_started()
            && let Some(p) = resp.interact_pointer_pos()
        {
            // egui reports drag_started only after ~6 px of travel — a fast
            // flick has left the knob by then. Hit-test the PRESS ORIGIN so
            // quick grabs don't fall through to box-select — and ANCHOR the
            // delta there too: anchoring at the current pointer discarded
            // the lead-in motion (a short drag left the edge unmoved).
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
            if let Some(h) = pick(origin) {
                self.workspace.masks_mut().mask_drag = Some((h, orig_at(origin)));
            }
        }
        if resp.dragged()
            && let (Some((h, last)), Some(p)) = (self.workspace.masks().mask_drag, resp.interact_pointer_pos())
        {
            let cur = orig_at(p);
            // The knob's geometry is the logic layer's (`logic/mask_edit`).
            if let Some(mask) = self.workspace.masks().sel_mask {
                self.apply_actions(vec![logic::Action::MaskEdit(
                    logic::mask_edit::MaskEditAction::Drag {
                        mask,
                        component: self.workspace.masks().sel_component,
                        handle: h,
                        last,
                        cur,
                    },
                )]);
            }
            self.workspace.masks_mut().mask_drag = Some((h, cur));
            // Masks are develop stages: the preview follows every drag frame.
            self.editor.session_mut().dirty = true;
            self.workspace.overlay_mut().overlay_stale = true;
        }
        if resp.drag_stopped() {
            self.workspace.masks_mut().mask_drag = None; // commit_if_settled turns the drag into ONE undo step
        }
        if self.workspace.masks().mask_drag.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if hover_h.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        self.workspace.masks().mask_drag.is_some() || hover_h.is_some()
    }

    /// Box-select on the After image: drag a rectangle to target a local edit;
    /// the normalized box is folded into the AI direction so it masks exactly
    /// there (mirrors the web region→mask prompt). A plain click — or a tiny
    /// drag — clears the selection. Coordinates are full-frame normalized (the
    /// AI mask space), mapped through the view transform.
    pub(crate) fn handle_region_select(&mut self, ui: &egui::Ui, resp: &egui::Response, xf: ViewXform) {
        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                // Anchor at the PRESS ORIGIN: egui reports drag_started only
                // after ~6 px of travel, so anchoring at the current pointer
                // shaved that lead-in off every box (small fast selections
                // could then die to the 0.02 minimum below).
                let s = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
                self.workspace.overlay_mut().region_drag = Some((s, p));
            }
        } else if resp.dragged() {
            if let (Some(p), Some((s, _))) = (resp.interact_pointer_pos(), self.workspace.overlay().region_drag) {
                self.workspace.overlay_mut().region_drag = Some((s, p));
            }
        } else if resp.drag_stopped() {
            if let Some((s, e)) = self.workspace.overlay_mut().region_drag.take() {
                // The region feeds the AI's mask prompt — ORIGINAL frame space.
                // ALL FOUR corners map (the same policy as serve's
                // region_to_original): under rotation/distortion the two
                // diagonal corners alone describe a different — sometimes
                // near-degenerate — box than the rectangle the user drew.
                let ((bw, bh), deg, dist) = self.editor.geom_ctx();
                let map = |p: egui::Pos2| {
                    let (nx, ny) = xf.to_norm(p);
                    view_norm_to_orig(nx, ny, (bw, bh), deg, &dist)
                };
                let corners = [
                    map(s),
                    map(e),
                    map(egui::pos2(s.x, e.y)),
                    map(egui::pos2(e.x, s.y)),
                ];
                self.apply_actions(vec![logic::Action::Region(logic::region::RegionAction::Select(corners))]);
            }
        } else if resp.clicked() && !resp.double_clicked() {
            // Remember what this click cleared: if it turns out to be the
            // first half of a Fit↔1:1 double-click, after_view restores it
            // (M18 — zooming must not eat the AI region).
            //
            // `&& !resp.double_clicked()`: egui sets `clicked` on the release
            // that COMPLETES the pair too, and after_view restores the region
            // earlier in this same frame — so without this the restore was
            // taken straight back out and the double-click still ate the
            // region. The fix was inert until this line existed.
            self.workspace.overlay_mut().region_restore = self.workspace.overlay_mut().region.take().map(|r| (r, Instant::now()));
        }
        // (Drawing lives in after_view so the box shows under every tool —
        // this handler only owns the gesture.)
    }

    /// The interactive crop overlay: darkened surround, thirds grid, corner +
    /// edge handles (aspect-constrained), move-inside, and drag OUTSIDE the
    /// box to rotate-straighten (the LR gesture set). The crop lives in
    /// `recipe.crop` (full-frame normalized) — exactly what the export render
    /// and the XMP already apply, so the tool adds no new data path.
    pub(crate) fn handle_crop(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        xf: ViewXform,
        tex_size: egui::Vec2,
    ) {
        // Pixel aspect ratio (w/h) requested by the preset; "原始" resolves here.
        // `tex_size` is the CURRENT after texture: right after a straighten
        // change with the develop still in flight it can be one frame stale,
        // and a preset applied in that instant derives against the previous
        // frame's dims (recorded residual, CX5-5 — recomputing inscribed
        // dims here would duplicate the engine's geometry; the next
        // interaction re-derives correctly).
        let aspect = CROP_ASPECTS[self.workspace.crop().aspect.min(CROP_ASPECTS.len() - 1)]
            .1
            .map(|r| if r == 0.0 { tex_size.x / tex_size.y.max(1.0) } else { r });
        // A preset picked in the panel re-derives the box NOW (LR applies the
        // aspect immediately, D13): largest same-centre fit at the new
        // ratio, shrunk to stay in frame. "Free" keeps the box untouched.
        if std::mem::take(&mut self.workspace.crop_mut().aspect_pending) {
            // The box is the logic layer's geometry (`logic::crop`); this view
            // keeps the pick, the handles and the overlay.
            let current = self.editor.doc().recipe.crop.map(|c| [c.left, c.top, c.right, c.bottom]);
            self.apply_actions(vec![logic::Action::Crop(logic::crop::CropAction::Preset {
                current,
                aspect_px: aspect,
                tex: (tex_size.x, tex_size.y),
            })]);
        }
        let cur = self.editor.doc().recipe
            .crop
            .map(|c| [c.left, c.top, c.right, c.bottom])
            .unwrap_or([0.0, 0.0, 1.0, 1.0]);

        // Handle order: 0=TL 1=TR 2=BL 3=BR, 4=move (inside), 5=T 6=B 7=L 8=R
        // edge midpoints, 9=rotate (anywhere outside the box).
        const HIT: f32 = 12.0; // handle pick radius, px — shared by drag + cursor
        let handle_pos = |c: &[f32; 4], k: u8| match k {
            0 => xf.to_screen(c[0], c[1]),
            1 => xf.to_screen(c[2], c[1]),
            2 => xf.to_screen(c[0], c[3]),
            3 => xf.to_screen(c[2], c[3]),
            5 => xf.to_screen((c[0] + c[2]) / 2.0, c[1]),
            6 => xf.to_screen((c[0] + c[2]) / 2.0, c[3]),
            7 => xf.to_screen(c[0], (c[1] + c[3]) / 2.0),
            _ => xf.to_screen(c[2], (c[1] + c[3]) / 2.0),
        };
        // Corners win over edges on a tiny box (checked first); inside = move;
        // anywhere else on the canvas = rotate-straighten, so the pick is
        // total — implicit pan is already suppressed while crop is armed.
        let pick_handle = |c: &[f32; 4], p: egui::Pos2| {
            (0u8..4)
                .chain(5u8..9)
                .find(|&k| handle_pos(c, k).distance(p) <= HIT)
                .or_else(|| {
                    let r = egui::Rect::from_min_max(
                        xf.to_screen(c[0], c[1]),
                        xf.to_screen(c[2], c[3]),
                    );
                    // A box flush with the canvas (a fresh full-frame crop)
                    // has no outside — the promised rotate-straighten gesture
                    // was unreachable. Only a FULLY flush box donates inner
                    // bands (moving it is a no-op anyway, D13); a box merely
                    // touching ONE canvas edge still has real outside to
                    // rotate from, and per-edge donation turned a move near
                    // that edge into a surprise rotate (M20).
                    const BAND: f32 = 16.0;
                    let mut inner = r.intersect(xf.rect);
                    let full_frame = r.min.x <= xf.rect.min.x + 1.0
                        && r.min.y <= xf.rect.min.y + 1.0
                        && r.max.x >= xf.rect.max.x - 1.0
                        && r.max.y >= xf.rect.max.y - 1.0;
                    if full_frame {
                        inner.min.x += BAND;
                        inner.min.y += BAND;
                        inner.max.x -= BAND;
                        inner.max.y -= BAND;
                    }
                    Some(if inner.contains(p) { 4 } else { 9 })
                })
        };
        // A stranded grab (Esc mid-drag, or leaving/re-entering crop mode)
        // must not resume against a stale anchor: no drag in flight ⇒ no grab.
        if !resp.dragged() && !resp.drag_started() {
            self.workspace.crop_mut().drag = None;
        }
        if resp.drag_started()
            && let Some(p) = resp.interact_pointer_pos()
        {
            // Hit-test the PRESS ORIGIN — drag_started fires only after ~6 px
            // of travel, and a fast corner grab has already left the handle.
            // The drag ANCHOR is the origin too: anchoring at the current
            // pointer discarded the lead-in motion (and let the dominant-axis
            // pick follow a small residual instead of the user's gesture).
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
            if let Some(h) = pick_handle(&cur, origin) {
                self.workspace.crop_mut().drag = Some((h, origin, cur, self.editor.doc().recipe.straighten_deg));
            }
        }
        if resp.dragged()
            && let (Some((h, start, orig, deg0)), Some(p)) =
                (self.workspace.crop().drag, resp.interact_pointer_pos())
        {
            if h == 9 {
                // Rotate-straighten: drag outside the box (LR). Angle around
                // the box centre, screen space — the WRAP and the ±45° clamp
                // are the logic layer's.
                let a = xf.to_screen(orig[0], orig[1]);
                let b = xf.to_screen(orig[2], orig[3]);
                let centre = egui::pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                let ang = |q: egui::Pos2| (q.y - centre.y).atan2(q.x - centre.x);
                let raw_delta_deg = (ang(p) - ang(start)).to_degrees();
                self.apply_actions(vec![logic::Action::Crop(logic::crop::CropAction::Rotate {
                    base_deg: deg0,
                    raw_delta_deg,
                })]);
            } else {
                let (sn, pn) = (xf.to_norm(start), xf.to_norm(p));
                self.apply_actions(vec![logic::Action::Crop(logic::crop::CropAction::Drag {
                    handle: h,
                    start: orig,
                    delta: (pn.0 - sn.0, pn.1 - sn.1),
                    aspect_px: aspect,
                    tex: (tex_size.x, tex_size.y),
                })]);
            }
        }
        if resp.drag_stopped() {
            self.workspace.crop_mut().drag = None;
        }

        // Cursor affordance: name the resize direction of the handle under
        // the pointer — or of the one being DRAGGED, since the pointer can
        // lag off a corner mid-drag. Runs after show_image's generic
        // crosshair set, and cursor_icon is last-write-wins, so this
        // overrides it exactly when a handle would take the drag.
        let c = self.editor.doc().recipe
            .crop
            .map(|c| [c.left, c.top, c.right, c.bottom])
            .unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let hover_handle = self.workspace.crop().drag
            .map(|(h, ..)| h)
            .or_else(|| resp.hover_pos().and_then(|p| pick_handle(&c, p)));
        if let Some(h) = hover_handle {
            ui.ctx().set_cursor_icon(match h {
                0 | 3 => egui::CursorIcon::ResizeNwSe, // TL/BR diagonal
                1 | 2 => egui::CursorIcon::ResizeNeSw, // TR/BL diagonal
                5 | 6 => egui::CursorIcon::ResizeVertical, // top/bottom edge
                7 | 8 => egui::CursorIcon::ResizeHorizontal, // left/right edge
                4 => egui::CursorIcon::Move,           // inside: move the window
                _ => egui::CursorIcon::Grab,           // outside: rotate-straighten
            });
        }

        // --- overlay: darkened surround + thirds + handles --------------------
        let p = ui.painter_at(xf.rect);
        // The TRUE box for guides + border (the painter clips): computing
        // the thirds on the viewport-intersected rect drew them at thirds of
        // the VISIBLE PORTION whenever zoom pushed part of the box
        // off-screen (CX5-8). The shading keeps the intersected rect — its
        // job is exactly the visible surround.
        let r_full =
            egui::Rect::from_min_max(xf.to_screen(c[0], c[1]), xf.to_screen(c[2], c[3]));
        let r = r_full.intersect(xf.rect);
        let dark = egui::Color32::from_black_alpha(140);
        let full = xf.rect;
        for shade in [
            egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, r.min.y)), // top
            egui::Rect::from_min_max(egui::pos2(full.min.x, r.max.y), full.max), // bottom
            egui::Rect::from_min_max(egui::pos2(full.min.x, r.min.y), egui::pos2(r.min.x, r.max.y)),
            egui::Rect::from_min_max(egui::pos2(r.max.x, r.min.y), egui::pos2(full.max.x, r.max.y)),
        ] {
            if shade.width() > 0.0 && shade.height() > 0.0 {
                p.rect_filled(shade, 0.0, dark);
            }
        }
        // O cycles the guide while cropping (LR): thirds → golden → off.
        let guide: &[f32] = match self.workspace.crop().grid {
            0 => &[1.0 / 3.0, 2.0 / 3.0],
            1 => &[0.381_966, 0.618_034], // 1−1/φ and 1/φ
            _ => &[],
        };
        let grid = egui::Stroke::new(1.0, egui::Color32::from_white_alpha(70));
        for &t in guide {
            p.line_segment(
                [
                    egui::pos2(r_full.min.x + t * r_full.width(), r_full.min.y),
                    egui::pos2(r_full.min.x + t * r_full.width(), r_full.max.y),
                ],
                grid,
            );
            p.line_segment(
                [
                    egui::pos2(r_full.min.x, r_full.min.y + t * r_full.height()),
                    egui::pos2(r_full.max.x, r_full.min.y + t * r_full.height()),
                ],
                grid,
            );
        }
        p.rect_stroke(r_full, 0.0, egui::Stroke::new(1.5, egui::Color32::WHITE), egui::StrokeKind::Middle);
        for k in 0..4u8 {
            p.rect_filled(
                egui::Rect::from_center_size(handle_pos(&c, k), egui::vec2(9.0, 9.0)),
                1.0,
                egui::Color32::WHITE,
            );
        }
        // Edge midpoints as slim pills, so the affordance reads as "this edge"
        // rather than a fifth corner.
        for k in [5u8, 6, 7, 8] {
            let sz = if k <= 6 { egui::vec2(15.0, 5.0) } else { egui::vec2(5.0, 15.0) };
            p.rect_filled(
                egui::Rect::from_center_size(handle_pos(&c, k), sz),
                1.0,
                egui::Color32::WHITE,
            );
        }
    }

    /// Place (or re-draw) a manual local-adjustment mask by dragging: the drag
    /// vector defines a linear gradient (press = full-strength side, LR's
    /// direction; Shift = axis lock) or the
    /// bounding box of a radial. Commits into `recipe.masks` — the SAME field
    /// the AI writes, so render + XMP need nothing new.
    pub(crate) fn handle_place_mask(&mut self, ui: &egui::Ui, resp: &egui::Response, xf: ViewXform) {
        let Some((kind, target)) = self.workspace.masks().placing_mask else { return };
        // Mask geometry lives in the ORIGINAL frame (the engine composites
        // masks before the geometric remap) — map pointer positions out of the view.
        let (dims, deg, dist) = self.editor.geom_ctx();
        // place_start holds VIEW-normalized coords (not original-frame): the
        // radial bbox below must map ALL FOUR view-space corners — under
        // rotation/distortion the two dragged diagonals alone can describe a
        // thin or zero-area original-space box (the region handler maps all
        // four for exactly this reason).
        if resp.drag_started()
            && let Some(p) = resp.interact_pointer_pos()
        {
            // Anchor at the PRESS ORIGIN: drag_started fires ~6 px in, and
            // that lead-in shortened every gradient / shifted tiny radials.
            let start = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
            self.workspace.masks_mut().place_start = Some(xf.to_norm(start));
        }
        let Some(sv) = self.workspace.masks().place_start else { return };
        let Some(p) = resp.interact_pointer_pos() else { return };
        let mut ev = xf.to_norm(p);
        // LR: Shift locks a linear gradient to horizontal / vertical.
        // Snapped in VIEW space (what the user sees) — under straighten the
        // original-frame vector is legitimately off-axis (D13).
        if kind == MaskKind::Linear && ui.input(|i| i.modifiers.shift) {
            if (ev.0 - sv.0).abs() >= (ev.1 - sv.1).abs() {
                ev.1 = sv.1;
            } else {
                ev.0 = sv.0;
            }
        }
        let geom = match kind {
            MaskKind::Linear => {
                // Endpoints map exactly — two points suffice for a gradient.
                // LR anchors the FULL effect at the press and fades toward
                // the release (D13): press = full side, release = zero side.
                let s = view_norm_to_orig(sv.0, sv.1, dims, deg, &dist);
                let e = view_norm_to_orig(ev.0, ev.1, dims, deg, &dist);
                autoshade::recipe::MaskGeometry::Linear {
                    zero_x: e.0,
                    zero_y: e.1,
                    full_x: s.0,
                    full_y: s.1,
                }
            }
            MaskKind::Radial => {
                // Under straighten/distortion the dragged view-rect has NO
                // exact preimage in the model — Radial carries no rotation —
                // so the committed area is the bbox of the mapped corners: a
                // SUPERSET of the drag (the effect covers everything pointed
                // at; the alternative, axis-preserving, would miss corners).
                // The selected-mask outline then shows the TRUE transformed
                // ellipse (parametric sampler in draw_masks) and the edge
                // handles move by deltas, so the approximation never
                // compounds. Identity when geometry is neutral.
                let corners = [(sv.0, sv.1), (ev.0, ev.1), (sv.0, ev.1), (ev.0, sv.1)]
                    .map(|(x, y)| view_norm_to_orig(x, y, dims, deg, &dist));
                let (mut l, mut t, mut r, mut b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for (x, y) in corners {
                    l = l.min(x);
                    t = t.min(y);
                    r = r.max(x);
                    b = b.max(y);
                }
                // A degenerate box is useless and invisible — pad to the same
                // MIN_SIZE floor the edge handles enforce.
                const MIN_SIZE: f32 = 0.01;
                if r - l < MIN_SIZE {
                    let c = (l + r) * 0.5;
                    l = c - MIN_SIZE * 0.5;
                    r = c + MIN_SIZE * 0.5;
                }
                if b - t < MIN_SIZE {
                    let c = (t + b) * 0.5;
                    t = c - MIN_SIZE * 0.5;
                    b = c + MIN_SIZE * 0.5;
                }
                autoshade::recipe::MaskGeometry::Radial {
                    top: t,
                    left: l,
                    bottom: b,
                    right: r,
                    feather: 0.5,
                    roundness: 0.0,
                    flipped: false,
                    angle: 0.0,
                    midpoint: 50.0,
                    mask_version: 2,
                }
            }
        };
        // Live placement preview: no owning adjustment yet → markers upright.
        draw_mask_overlay(ui, xf, &geom_to_view(&geom, dims, deg, &dist), false, self.lang);
        if resp.drag_stopped() {
            // The commit's MEANING is the logic layer's (redraw keeps the
            // tuned state, a component joins its mask, a fresh drag creates
            // one); this handler keeps the drag itself.
            let place = match target {
                PlaceTarget::NewMask => logic::mask_place::Place::NewMask,
                PlaceTarget::Redraw(i) => logic::mask_place::Place::Redraw(i),
                PlaceTarget::Component(i, mode) => logic::mask_place::Place::Component(i, mode),
            };
            let n = (self.editor.doc().recipe.masks.len() + 1).to_string();
            let name = trf(self.lang, K_CANVAS_MANUAL, &[("n", &n)]);
            let committed = logic::mask_place::commit(&mut self.editor.doc_mut().recipe, place, geom, name);
            match committed.selection {
                logic::mask_place::Selection::Keep => {}
                logic::mask_place::Selection::Mask(i) => {
                    self.workspace.masks_mut().sel_mask = Some(i);
                    self.workspace.masks_mut().sel_component = None;
                }
                logic::mask_place::Selection::Component(i, c) => {
                    self.workspace.masks_mut().sel_mask = Some(i);
                    self.workspace.masks_mut().sel_component = Some(c);
                }
            }
            self.run_effects(committed.effects);
            self.workspace.masks_mut().placing_mask = None;
            self.workspace.masks_mut().place_start = None;
            self.workspace.overlay_mut().overlay_stale = true;
        }
    }

}
