//! What a panel may touch: a projection of the aggregates, never the app.
//!
//! A panel draws with the edit (the widgets that own a recipe vector — the
//! curve editor — mutate it), the view state, the language and the test seams.
//! It cannot reach the workers, the services or another panel's methods: the
//! type simply does not carry them.

use crate::*;
use crate::model::AiPrefs;
use crate::panels::retouch::{BRUSH_R_MAX, BRUSH_R_MIN};
use crate::sliders::*;
use crate::panels::grade_wheel::{grade_wheel, GRADE_VIEWS};
use autoshade::advisor::catalogue::{family_is_active, CONTROL_FAMILIES};
use crate::panels::develop_widgets::parametric_curve;
use crate::panels::ai::{is_working_note, note_args, thinking_notes};

pub(crate) struct PanelView<'a, 'b> {
    pub(crate) editor: &'a mut Editor,
    pub(crate) workspace: &'a mut WorkspaceView,
    pub(crate) lang: Lang,
    pub(crate) aip: &'a mut AiPrefs,
    /// The frame's action sink: a panel reports intents, the app runs them.
    pub(crate) actions: &'b mut Vec<logic::Action>,
    #[cfg(test)]
    pub(crate) seams: &'a mut TestSeams,
}

impl PanelView<'_, '_> {
    /// The card's ACTION buttons — ＋ archive, ▣ apply-to-Original, ✕ delete
    /// — for whichever surface is drawing the card (R24-5).
    ///
    /// Extracted from `variant_strip` verbatim when the Versions section's
    /// edit-state list grew the same three buttons: a card action must mean
    /// one thing and be implemented once, or the two lists answer 「what does
    /// ✕ do here」 differently the first time either is touched. The strip
    /// keeps the rename box (see [`VariantSurface`]).
    ///
    /// Returns the deferred action; the caller performs it through
    /// `dispatch_variant_action` after the layout closure has released `self`.
    // `surface` decides only which headless seam records the row, so it is
    // genuinely unused in a release build — the alternative (two near-copies
    // of this function) is the thing the extraction exists to remove.
    #[cfg_attr(not(test), allow(unused_variables))]
    pub(crate) fn variant_card_buttons(
        &mut self,
        ui: &mut egui::Ui,
        i: usize,
        active: bool,
        busy: bool,
        surface: VariantSurface,
    ) -> Option<VariantAction> {
        let lang = self.lang;
        let kind = self.editor.doc().variants[i].kind;
        let mut act: Option<VariantAction> = None;
        // Which glyphs this row actually laid out — the list surface's test
        // seam, since it has no rects of its own to assert on.
        #[cfg(test)]
        let mut drawn: Vec<&str> = Vec::new();
        // R24-2 (user decision ②, in place of save-as-you-go snapshots): a
        // one-click archive entry point ON the card whose develop it
        // snapshots. Active card only — the button acts on the LIVE canvas,
        // and offering it on a background card would promise a snapshot of
        // something else. A row-height glyph square in the existing label row
        // (R38): the row is already interact_size.y tall, so the strip's
        // card-height arithmetic (and its geometry test) is untouched.
        // Inert (visibly, not silently) while a photo is still opening — the
        // canvas then still holds the OUTGOING photo's develop, and the
        // snapshot would land it in the incoming photo's store. Same rule the
        // Versions section applies to its own copy.
        if active {
            #[cfg(test)]
            drawn.push("＋");
            if glyph(ui, !busy, "＋")
                .on_hover_text(tr(lang, K_DEVELOP_SNAPSHOT_THIS_CARD_S_DEVELOP_ONLY_AS_A_NUMBERED))
                .clicked()
            {
                act = Some(VariantAction::SaveVersion);
            }
        }
        // R24-3 (#7) 「apply to Original」, on the ACTIVE card for the same
        // reason ＋ is: it copies the LIVE canvas. Two concepts the hover keeps
        // apart — it overwrites the ▣ Original CARD's develop parameters (its
        // pixels stay, this card stays), and Ctrl+S is what afterwards makes
        // that card's develop the photo's SAVED develop.
        // A pixel-state card shows the button DISABLED with the reason rather
        // than hiding it (the R24-2 save_version judgement, same remedy): its
        // look is in its raster, so there is nothing parametric to copy.
        // Hidden entirely when this card IS the negative, or when the strip
        // has no Original card to apply onto.
        if active && kind != VariantKind::Original && self.editor.original_index().is_some() {
            let can = kind.is_source_based();
            let hover = if can {
                tr(lang, K_DEVELOP_COPY_THIS_VARIANT_S_DEVELOP_ONTO_THE_ORIGINAL_CA)
            } else if kind == VariantKind::Edited {
                tr(lang, K_DEVELOP_THIS_CARD_S_DEVELOP_IS_TUNED_OVER_AI_GENERATED_P)
            } else {
                tr(lang, K_DEVELOP_A_GENERATED_VARIANT_S_LOOK_LIVES_IN_ITS_PIXELS_T)
            };
            let resp = glyph(ui, can && !busy, "▣").on_hover_text(hover);
            #[cfg(test)]
            drawn.push(if can && !busy { "▣" } else { "▣(off)" });
            #[cfg(test)]
            if surface == VariantSurface::Strip {
                // The STRIP owns this seam: a full frame draws both surfaces,
                // and letting the list overwrite it would silently retarget
                // every existing assertion at the other widget.
                self.seams.strip_apply = Some((resp.rect, can && !busy));
            }
            if resp.clicked() {
                act = Some(VariantAction::Apply(i));
            }
        }
        // Any variant except the sole Original can be dropped.
        // ✕ — the ONE close/cancel glyph app-wide (the old bare × was the
        // fourth variant of it). ARMED (R24-4): the first click asks, the
        // second deletes — the button says which, because a deleted card
        // cannot be brought back (unlike a version, whose number the registry
        // keeps burned rather than the card recoverable).
        let armed = self.workspace.cards().variant_delete_confirm == Some(i);
        if self.editor.doc().variants.len() > 1
            && kind != VariantKind::Original
            && glyph(ui, true, if armed { "✕?" } else { "✕" })
                .on_hover_text(if armed {
                    tr(lang, K_DEVELOP_CLICK_AGAIN_TO_DELETE_THIS_VARIANT_IT_CANNOT_BE)
                } else {
                    tr(lang, K_DEVELOP_DELETE_THIS_VARIANT)
                })
                .clicked()
        {
            act = Some(VariantAction::Delete(i));
        }
        #[cfg(test)]
        if self.editor.doc().variants.len() > 1 && kind != VariantKind::Original {
            drawn.push(if armed { "✕?" } else { "✕" });
        }
        #[cfg(test)]
        if surface == VariantSurface::List {
            self.seams.edit_list_actions.push(format!("{i}:{}", drawn.join("")));
        }
        act
    }
    /// The selected entry developed on the open photo, as a texture + its
    /// pixel size. Rebuilt only when its key changes — the entry's file stamp,
    /// the photo, or the base pixels' allocation — so clicking through the
    /// list pays one downscale plus one small develop per entry, never a
    /// full-frame render.
    pub(crate) fn recipe_preview_tex(
        &mut self,
        ctx: &egui::Context,
        name: &str,
    ) -> Option<(egui::TextureId, egui::Vec2)> {
        let src = self.editor.photo().src_path.clone()?;
        let base = self.editor.photo().base_preview.clone()?;
        let entry = autoshade::recipe_library::read(name).ok()?;
        let stamp = std::fs::metadata(autoshade::recipe_library::entry_path(name))
            .and_then(|m| m.modified())
            .ok();
        let key = format!(
            "{name}|{}|{stamp:?}|{:p}",
            src.display(),
            std::sync::Arc::as_ptr(&base)
        );
        if let Some(p) = &self.workspace.rlib().recipe_preview
            && p.key == key
        {
            return Some((p.texture.id(), p.texture.size_vec2()));
        }
        let plate = autoshade::render::Plate::thumbnail_of(&base, 420);
        // What APPLYING would show: the target's own calibration, the
        // library entry's edit — the same shaping the paste path runs.
        let mut r = crate::persist::paste_recipe_for(&src, &entry);
        r.clamp();
        let img = autoshade::render::develop_preview(plate.shared().as_ref(), &r);
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        let texture = ctx.load_texture(
            format!("recipe-preview:{key}"),
            color,
            egui::TextureOptions::LINEAR,
        );
        let shown = (texture.id(), texture.size_vec2());
        self.workspace.rlib_mut().recipe_preview = Some(RecipePreview { key, texture });
        Some(shown)
    }
    /// The READ side: whether an analysis searches the library, and whether
    /// the references also go to the model as pictures.
    pub(crate) fn ai_library_use(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let built = matches!(self.workspace.ai().style_info, Some(autoshade::library::Summary::Built { .. }));
        ui.checkbox(&mut self.aip.style_embed, tr(lang, K_AI_SEARCH_THE_LIBRARY_WHEN_ANALYSING))
            .on_hover_text(tr(lang,
                K_AI_EACH_ANALYSIS_MEASURES_THIS_PHOTO_WITH_A_LOCAL_M,
            ));
        if built && !self.aip.style_embed {
            ui.label(
                Text::new(tr(lang, K_AI_BUILT_BUT_NOT_SEARCHED_THE_SWITCH_ABOVE_IS_OFF))
                    .color(ui.visuals().warn_fg_color)
                    .small(),
            );
        }
        ui.add_enabled_ui(built && self.aip.style_embed, |ui| {
            ui.checkbox(&mut self.aip.send_style_ref_image, tr(lang, K_AI_ALSO_SHOW_THE_MODEL_THE_REFERENCE_PHOTOS))
                .on_hover_text(if built {
                    tr(lang,
                        K_AI_WILL_UPLOAD_UP_TO_3_MORE_IMAGES_PER_ANALYSIS_CAL,
                    )
                } else {
                    tr(lang, K_AI_BUILD_A_REFERENCE_LIBRARY_FIRST_THERE_IS_NO_REFE)
                });
        });
    }
    /// Which library, how big, how old, and where — or why there is none.
    pub(crate) fn ai_library_status(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let line = match &self.workspace.ai().style_info {
            Some(autoshade::library::Summary::Built { photos, phrased, with_settings, dir, age }) => {
                let mut t = trf(
                    lang,
                    K_AI_FINISHED_PHOTOS_FROM_WITH_LOOK_PHRASES,
                    &[("n", &photos.to_string()), ("path", dir), ("m", &phrased.to_string())],
                );
                if *with_settings > 0 {
                    t.push_str(" · ");
                    t.push_str(&trf(lang, K_AI_WITH_RAW_SETTINGS, &[("k", &with_settings.to_string())]));
                }
                if let Some(age) = age {
                    // Coarse buckets, no calendar arithmetic: "how stale is my
                    // library" is answered by hours or days, and both spellings
                    // avoid a plural rule.
                    const HOUR: u64 = 3600;
                    let secs = age.as_secs();
                    t.push_str(" · ");
                    t.push_str(&if secs < 48 * HOUR {
                        trf(lang, K_AI_BUILT_H_AGO, &[("hours", &(secs / HOUR).to_string())])
                    } else {
                        trf(lang, K_AI_BUILT_D_AGO, &[("days", &(secs / (24 * HOUR)).to_string())])
                    });
                }
                Text::new(t).small()
            }
            Some(autoshade::library::Summary::Unusable(err)) => Text::new(trf(
                lang,
                K_AI_THE_REFERENCE_LIBRARY_COULD_NOT_BE_READ_REBUILD,
                &[("err", err)],
            ))
            .color(ui.visuals().warn_fg_color)
            .small(),
            Some(autoshade::library::Summary::Absent) => Text::new(tr(lang,
                K_AI_NO_REFERENCE_LIBRARY_YET_THE_STYLE_SLIDER_ABOVE,
            ))
            .small(),
            None => Text::new(tr(lang, K_AI_READING_THE_REFERENCE_LIBRARY)).weak().small(),
        };
        ui.label(line).on_hover_text(trf(
            lang,
            K_AI_LIBRARY_FILE,
            &[("path", &abs_display(&autoshade::library::path()))],
        ));
    }
    /// Bake the brush session: the greyscale weight buffer becomes a freshly
    /// CLAIMED raster (the input file is never mutated — saved recipes and
    /// version snapshots keep rendering what they rendered), then the target
    /// mask repoints at it, or a new Bitmap mask is pushed.
    pub(crate) fn commit_mask_brush(&mut self) {
        let Some((target, _)) = self.workspace.masks().mask_brush else { return };
        let Some(gray) = self.workspace.masks().mask_brush_gray.clone() else { return };
        let Some(src) = self.editor.photo().src_path.clone() else { return };
        let lang = self.lang;
        if target.is_none() {
            if gray.pixels().all(|p| p[0] == 0) {
                let t = tr(lang, K_GUI_NOTHING_PAINTED_YET_DRAG_ON_THE_IMAGE_FIRST);
                self.workspace.toast(ToastKind::Error, t.to_string());
                return;
            }
            if self.editor.doc().recipe.masks.len() >= 64 {
                let t = tr(lang, K_ACTIONS_MASK_LIMIT_REACHED_64_DELETE_ONE_FIRST);
                self.workspace.toast(ToastKind::Error, t.to_string());
                return;
            }
        }
        let written = autoshade::store::claim_raster(&src, "mask-brush")
            .map_err(anyhow::Error::from)
            .and_then(|p| {
                gray.save(&p)?;
                // Durable BEFORE the recipe references it (L03): the JSON
                // commits through fsync, and a raster still in the page
                // cache leaves a saved develop pointing at pixels a power
                // cut never wrote.
                autoshade::store::durable_adopt(&p)?;
                Ok(p)
            });
        match written {
            Ok(path) => {
                let path_s = path.to_string_lossy().into_owned();
                match target {
                    Some(i) if i < self.editor.doc().recipe.masks.len() => {
                        // REPOINT, never re-shape. A zone's geometry is the
                        // Select Sky component the sidecar carries; rewriting
                        // it as a `Bitmap` here would silently take the mask
                        // back OUT of the Lightroom sidecar it now rides in,
                        // as the reward for feathering it. A geometry with no
                        // raster at all has nothing to repoint, so for that one
                        // the bake still MAKES the raster.
                        match autoshade::render::geometry_raster_path_mut(
                            &mut self.editor.doc_mut().recipe.masks[i].mask,
                        ) {
                            Some(slot) => *slot = path_s,
                            None => {
                                self.editor.doc_mut().recipe.masks[i].mask = MaskGeometry::Bitmap { path: path_s }
                            }
                        }
                        self.workspace.masks_mut().sel_mask = Some(i);
                        self.workspace.set_status(tr(lang, K_GUI_MASK_RASTER_UPDATED_ITS_ADJUSTMENTS_NOW_APPLY_TO).into());
                    }
                    _ => {
                        let n = self.editor.doc().recipe.masks.len();
                        self.editor.doc_mut().recipe.masks.push(autoshade::recipe::LocalAdjustment {
                            mask: MaskGeometry::Bitmap { path: path_s },
                            name: trf(lang, K_GUI_BRUSH, &[("n", &(n + 1).to_string())]),
                            ..Default::default()
                        });
                        self.workspace.masks_mut().sel_mask = Some(n);
                        self.workspace.set_status(tr(lang, K_GUI_BRUSH_MASK_CREATED_PULL_ITS_SLIDERS_IN_LOCAL_MAS).into());
                    }
                }
                self.workspace.masks_mut().sel_component = None;
                self.editor.session_mut().dirty = true;
                self.workspace.overlay_mut().overlay_stale = true;
                self.disarm_tools(); // ends the session and clears the canvas
            }
            Err(e) => {
                let t = trf(
                    lang,
                    K_GUI_COULD_NOT_SAVE_THE_BRUSH_MASK,
                    &[("err", &e.to_string())],
                );
                self.workspace.toast(ToastKind::Error, t);
            }
        }
    }

    /// The three create tools, the brush-session controls and the AI
    /// segmentation verbs: the panel's "start a mask" half, above the list.
    pub(crate) fn dev_mask_create_row(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        // R38: three equal cells — three free-width toggles in a
        // non-wrapping row overran the default panel width in English.
        ui.horizontal(|ui| {
            let cell = columns(ui, 3);
            let lin_armed = matches!(self.workspace.masks().placing_mask, Some((MaskKind::Linear, PlaceTarget::NewMask)));
            if toggle_in(ui, cell, lin_armed, tr(lang, K_DEVELOP_LINEAR__2)).on_hover_text(tr(lang, K_DEVELOP_DRAG_ON_THE_IMAGE_START_FULLY_APPLIED_SIDE_END_U)).clicked() {
                self.disarm_tools();
                if !lin_armed {
                    self.workspace.masks_mut().placing_mask = Some((MaskKind::Linear, PlaceTarget::NewMask));
                    self.workspace.set_status(tr(lang, K_APP_DRAG_ON_THE_IMAGE_TO_DRAW_A_LINEAR_GRADIENT_STAR).into());
                }
            }
            let rad_armed = matches!(self.workspace.masks().placing_mask, Some((MaskKind::Radial, PlaceTarget::NewMask)));
            if toggle_in(ui, cell, rad_armed, tr(lang, K_DEVELOP_RADIAL__2)).on_hover_text(tr(lang, K_DEVELOP_DRAG_ON_THE_IMAGE_TO_DRAW_AN_ELLIPTICAL_AREA)).clicked() {
                self.disarm_tools();
                if !rad_armed {
                    self.workspace.masks_mut().placing_mask = Some((MaskKind::Radial, PlaceTarget::NewMask));
                    self.workspace.set_status(tr(lang, K_APP_DRAG_ON_THE_IMAGE_TO_DRAW_A_RADIAL_ELLIPTICAL_AR).into());
                }
            }
            // LR's most-used local tool, finally: free-form brush →
            // Bitmap mask (the raster carrier + bilinear sampler + cache
            // all pre-existed; this wires the paint canvas into
            // recipe.masks — see start_mask_brush / commit_mask_brush).
            let brush_armed = matches!(self.workspace.masks().mask_brush, Some((None, _)));
            if toggle_in(ui, cell, brush_armed, tr(lang, K_DEVELOP_BRUSH__2))
                .on_hover_text(tr(lang, K_DEVELOP_PAINT_A_FREE_FORM_MASK_DRAG_THE_BRUSH_SIZE_SLIDE))
                .clicked()
            {
                if brush_armed {
                    self.disarm_tools();
                } else if self.editor.photo().base_preview.is_some() {
                    self.act_all(vec![logic::Action::Develop(
                        logic::develop::DevelopAction::MaskBrush(None),
                    )]);
                }
            }
        });
        // Brush-session controls (create OR raster-edit): erase toggle +
        // the bake/cancel pair. Lives up here so it is visible whichever
        // row armed the session.
        if let Some((target, erase)) = self.workspace.masks().mask_brush {
            ui.horizontal(|ui| {
                let cell = columns(ui, 3);
                if toggle_in(ui, cell, erase, tr(lang, K_DEVELOP_ERASE))
                    .on_hover_text(tr(lang, K_DEVELOP_STROKES_REMOVE_FROM_THE_SELECTION_INSTEAD_OF_ADD))
                    .clicked()
                {
                    self.workspace.masks_mut().mask_brush = Some((target, !erase));
                }
                if primary_in(ui, cell, true, tr(lang, K_DEVELOP_APPLY)).clicked() {
                    self.commit_mask_brush();
                }
                if action_in(ui, cell, true, tr(lang, K_APP_CANCEL)).clicked() {
                    self.disarm_tools();
                }
            });
            // #9: the radius belongs AT THE TOOL. This is the same
            // `self.workspace.masks().brush` the Retouch panel's slider drives (and the
            // `[` / `]` keys) — one session block covers both arms, the
            // 「＋ Brush」 above and 「🖌 Edit raster」 down in the mask row.
            // Outside the row closure: that closure holds `&mut self`.
            self.brush_size_slider(ui);
        }
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_masks(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        // Geometry ends here — local adjustments and management below.
        // (Group boundary — see the Detail section's comment.)
        // #14b, group 3: masks and the pixel-level tools they reach.
        group_caption(ui, tr(lang, K_DEVELOP_LOCAL_PIXEL));
        // --- 局部调整: manual masks — the SAME recipe.masks the AI writes -----
        let n_masks = self.editor.doc().recipe.masks.len();
        let n_masks_s = n_masks.to_string();
        section_fold(ui, "sec_local", section_title(
            &trf(lang, K_DEVELOP_LOCAL_MASKS, &[("n", &n_masks_s)]),
            self.editor.masks_section_active(),
        ), false, false, |ui| {
            self.dev_mask_create_row(ui);
            self.dev_mask_ai_verbs(ui, n_masks);
            // Mask list: click to select (shows overlay + sliders), 🗑 deletes;
            // HOVERING a row previews that mask's coverage without selecting.
            // hover_mask lives ONE frame: update() takes it each frame and this
            // list re-sets it while the cursor is on a row — so leaving the
            // panel, collapsing this section or switching photos all fall back
            // to the selection with no stale index to chase.
            // Reorder is a dedicated ☰ grip per row (the ONLY drag source),
            // NOT the row itself: `dnd_drag_source` overlays its contents with
            // a drag-sensing interact (grab cursor), so wrapping the whole row
            // made the slightest press-move float the row instead of selecting
            // it (user-reported "click moves instead of selects"). Grip drags,
            // row body clicks — the drop TARGET stays the whole row. egui
            // clears the payload on release/Esc itself, and while a drag is in
            // flight `hovered()` is false everywhere, so the hover preview
            // pauses instead of churning the coverage overlay.
            let mut delete: Option<usize> = None;
            let mut toggle_eye: Option<usize> = None;
            let mut dropped: Option<(usize, usize)> = None; // (from, insert-before)
            for i in 0..n_masks {
                let row_resp = ui
                    .horizontal(|ui| {
                        let m = &self.editor.doc().recipe.masks[i];
                        let kind = match m.range {
                            Some(RangeMask::Luminance { .. }) => {
                                tr(self.lang, K_DEVELOP_LUMINANCE_RANGE)
                            }
                            Some(RangeMask::Color { .. }) => tr(self.lang, K_DEVELOP_COLOR_RANGE),
                            None => match m.mask {
                                MaskGeometry::Linear { .. } => tr(self.lang, K_DEVELOP_LINEAR),
                                MaskGeometry::Shaped { .. } => tr(self.lang, K_DEVELOP_SHAPED_GRADIENT),
                                MaskGeometry::Radial { .. } => tr(self.lang, K_DEVELOP_RADIAL),
                                MaskGeometry::Bitmap { .. } => tr(self.lang, K_DEVELOP_BITMAP),
                                MaskGeometry::Brush { .. } => tr(self.lang, K_DEVELOP_BRUSH),
                                // Named "AI" and never "Sky"/"Subject": the row
                                // must not read as Lightroom's own mask. What it
                                // draws is our segmenter's re-derivation, and the
                                // overlay badge plus the import line carry the
                                // rest of that sentence (R27 Batch-5).
                                MaskGeometry::AiMask { .. } => tr(self.lang, K_DEVELOP_AI),
                            },
                        };
                        // A user-given name wins; else a reverse-fit zone shows
                        // its localised role label; else the generic placeholder.
                        let base: &str = if !m.name.is_empty() {
                            m.name.as_str()
                        } else if let Some(en) = m.role.en_name() {
                            tr(self.lang, en)
                        } else {
                            tr(self.lang, K_DEVELOP_MASK)
                        };
                        // The activity dot IS the engine's own rule
                        // (util::mask_active → render::engine_active): a parked
                        // mask looks parked, a working one shows ● — a 64-row
                        // list where the two were indistinguishable was a real
                        // navigation cost. The SECTION header reads the same
                        // helper over the whole list (masks_section_active), so
                        // the two dots cannot drift apart again (R22 #16).
                        let active = mask_active(m);
                        let enabled = m.enabled;
                        // L08: a dead raster renders INERT (the engine skips
                        // it with a stderr-only warning) while this row still
                        // read "enabled" — the ⚠ puts the fact on the row.
                        // Muted/parked masks skip the probe: nothing renders.
                        let dead = if active {
                            autoshade::render::dead_bitmap_rasters(m)
                        } else {
                            Vec::new()
                        };
                        let label = format!(
                            "{base} · {kind}{}{}",
                            if active { "  ●" } else { "" },
                            if dead.is_empty() { "" } else { "  ⚠" }
                        );
                        let label = if enabled {
                            Text::new(label)
                        } else {
                            Text::new(label).weak()
                        };
                        ui.dnd_drag_source(ui.id().with(("mask_row", i)), i, |ui| {
                            ui.label("☰");
                        })
                        .response
                        .on_hover_text(tr(self.lang, K_DEVELOP_DRAG_TO_REORDER));
                        // The eye: Lightroom's lossless mute. Amount-to-0 as a
                        // mute destroyed the tuned value; this keeps it.
                        if glyph_toggle(ui, enabled, "👁")
                            .on_hover_text(tr(
                                self.lang,
                                K_DEVELOP_SHOW_MUTE_THIS_MASK_WITHOUT_LOSING_ITS_SETTINGS,
                            ))
                            .clicked()
                        {
                            toggle_eye = Some(i);
                        }
                        let mut row = ui.selectable_label(self.workspace.masks().sel_mask == Some(i), label);
                        if !dead.is_empty() {
                            row = row.on_hover_text(trf(
                                self.lang,
                                K_DEVELOP_BITMAP_RASTER_UNREADABLE_THIS_MASK_CURRENTLY_HAS,
                                &[("list", &dead.join(", "))],
                            ));
                        }
                        if row.hovered() {
                            self.workspace.overlay_mut().hover_mask = Some(i);
                        }
                        if row.clicked() {
                            self.workspace.masks_mut().sel_mask =
                                if self.workspace.masks().sel_mask == Some(i) { None } else { Some(i) };
                            // Component selection belongs to ONE mask's list.
                            self.workspace.masks_mut().sel_component = None;
                            self.workspace.overlay_mut().overlay_stale = true; // coverage follows the selection
                            // EVERY index-armed tool follows the selection —
                            // the colour sampler was fixed first, then the
                            // same class recurred for ↻ Redraw /
                            // add-component / the raster-edit brush, whose
                            // arming indicators all live inside the
                            // selected-mask block (L06#3). The single owner
                            // sits beside remap_mask_indices; the brush
                            // teardown discards user paint, so it is said.
                            if self.workspace.disarm_selection_bound_tools(self.workspace.masks().sel_mask) {
                                let t = tr(
                                    self.lang,
                                    K_ACTIONS_THE_RASTER_EDIT_BRUSH_SESSION_ENDED_YOU_SELECTED,
                                );
                                self.workspace.toast(ToastKind::Error, t.to_string());
                            }
                        }
                        if glyph(ui, true, "🗑")
                            .on_hover_text(tr(self.lang, K_DEVELOP_DELETE_THIS_MASK_ITS_STACK_ORDER_SHIFTS_THE_ONES))
                            .clicked()
                        {
                            delete = Some(i);
                        }
                    })
                    .response;
                // A grip being dragged over this row: mark the insertion edge
                // (above/below the row midline) and take the drop. The whole
                // row rect is the drop zone, so aiming needs no precision.
                if let (Some(from), Some(p)) =
                    (row_resp.dnd_hover_payload::<usize>(), ui.ctx().pointer_interact_pos())
                {
                    let below = p.y > row_resp.rect.center().y;
                    let y = if below { row_resp.rect.max.y } else { row_resp.rect.min.y };
                    ui.painter().hline(
                        row_resp.rect.x_range(),
                        y,
                        egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
                    );
                    let insert = if below { i + 1 } else { i };
                    if row_resp.dnd_release_payload::<usize>().is_some() {
                        dropped = Some((*from, insert));
                    }
                }
            }
            if let Some((from, insert)) = dropped
                // Bounds re-checked at APPLY time: the payload index was
                // captured at drag start, and the list can shrink mid-drag
                // (Ctrl+Z, an async AI result) — a stale `from` would panic
                // in Vec::remove.
                && from < self.editor.doc().recipe.masks.len()
                && insert <= self.editor.doc().recipe.masks.len()
                && insert != from
                && insert != from + 1
            {
                let (to, remap) = reorder_move(from, insert);
                crate::logic::command::apply(
                    &mut self.editor.doc_mut().recipe,
                    crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::Reorder { from, to }),
                );
                self.workspace.remap_mask_indices(|s| Some(remap(s)));
                self.workspace.overlay_mut().overlay_stale = true;
                changed = true;
            }
            if let Some(i) = toggle_eye {
                // A recipe mutation like any slider: develop + overlay follow,
                // and commit_if_settled makes it one undo step. Through the
                // command layer like the develop form's rows.
                crate::logic::command::apply(
                    &mut self.editor.doc_mut().recipe,
                    crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::ToggleEnabled { index: i }),
                );
                self.workspace.overlay_mut().overlay_stale = true;
                changed = true;
            }
            if let Some(i) = delete {
                crate::logic::command::apply(
                    &mut self.editor.doc_mut().recipe,
                    crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::Delete { index: i }),
                );
                self.workspace.remap_mask_indices(|s| {
                    if s == i {
                        None
                    } else if s > i {
                        Some(s - 1)
                    } else {
                        Some(s)
                    }
                });
                self.workspace.overlay_mut().overlay_stale = true;
                changed = true;
            }
            changed |= self.dev_mask_colour_field(ui);
            changed |= self.dev_mask_selected_or_hint(ui, n_masks);
        });
        changed
    }
    pub(crate) fn brush_size_slider(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        // The band the slider occupies, read off the layout cursor before and
        // after — a wrapper (scope/horizontal) would be one more thing that
        // can move the geometry the width tests pin. Test seam only.
        #[cfg(test)]
        let before = ui.cursor();
        // The radius is live tool state, never a recipe field: no dirty flag,
        // no redevelop — a change only flashes the ring on the canvas.
        let mut radius = self.workspace.masks().brush;
        if slider_hinted(
            ui,
            lang,
            tr(lang, K_RETOUCH_BRUSH_SIZE),
            &mut radius,
            BRUSH_R_MIN,
            BRUSH_R_MAX,
            30.0,
            tr(lang, K_RETOUCH_ONE_RADIUS_FOR_EVERY_BRUSH_THE_MASK_BRUSH_AND_TH),
        ) {
            let now = ui.input(|i| i.time);
            self.workspace.set_brush(radius, now);
        }
        // Feather and Flow (2026-10-13): Lightroom's two brush dials, shared
        // by every brush like the radius. Feather fades the disc's outer part
        // (the inner ring on the canvas marks where the fade starts); Flow is
        // how much one pass lays down, so a low flow builds up stroke by
        // stroke. The defaults are the hard full-strength brush of before.
        slider_pct_hinted(
            ui,
            lang,
            tr(lang, K_DEVELOP_FEATHER),
            &mut self.workspace.masks_mut().brush_feather,
            1.0,
            0.0,
            tr(lang, K_RETOUCH_HOW_SOFT_THE_BRUSH_EDGE_IS_0_A_HARD_EDGE_100_FAD),
        );
        slider_pct_hinted(
            ui,
            lang,
            tr(lang, K_RETOUCH_FLOW),
            &mut self.workspace.masks_mut().brush_flow,
            1.0,
            1.0,
            tr(lang, K_RETOUCH_HOW_MUCH_ONE_PASS_LAYS_DOWN_100_FULL_STRENGTH_AT),
        );
        #[cfg(test)]
        {
            // x from the Ui's USED region (cursor().max.x can be infinite in an
            // unbounded layout), y from the cursor the slider just advanced.
            let used = ui.min_rect();
            self.seams.brush_slider_rect = Some(egui::Rect::from_min_max(
                egui::pos2(used.left(), before.top()),
                egui::pos2(used.right(), ui.cursor().top()),
            ));
        }
    }
    /// The selected mask's geometry half: the name/verb row, the radial knobs,
    /// the raster tools, the composed shapes and the range editor — every edit
    /// up to and including Amount, i.e. the edits that can move the coverage
    /// wash. The caller invalidates the overlay when this returns true.
    pub(crate) fn dev_mask_geometry_and_range(&mut self, ui: &mut egui::Ui, i: usize) -> bool {
        let mut changed = self.dev_mask_verbs_and_geometry(ui, i);
        changed |= self.dev_mask_shapes(ui, i);
        changed |= self.dev_mask_range_and_amount(ui, i);
        changed
    }

    /// The selected mask's detail block, or the explainer when there are no
    /// masks, or the "select a mask" hint when the list is non-empty and
    /// nothing is selected. All three arms used to live inline in the section.
    pub(crate) fn dev_mask_selected_or_hint(&mut self, ui: &mut egui::Ui, n_masks: usize) -> bool {
        let lang = self.lang;
        let mut changed = false;
        // Selected mask: its full slider set.
        if let Some(i) = self.workspace.masks().sel_mask.filter(|&i| i < self.editor.doc().recipe.masks.len()) {
            let coverage = self.dev_mask_geometry_and_range(ui, i);
            // Any edit up to here — geometry, range, Amount — can move the
            // coverage wash (the overlay key carries them all); the key
            // compare inside refresh_mask_overlay dedupes rebuilds, so
            // over-flagging is free. The tone sliders below are not coverage
            // and deliberately stay out.
            if coverage {
                self.workspace.overlay_mut().overlay_stale = true;
            }
            changed |= coverage;
            changed |= self.dev_mask_adjustments(ui, i);
        } else if n_masks == 0 {
            explainer(ui, lang, tr(lang, K_DEVELOP_LIGHTROOM_STYLE_LOCAL_ADJUSTMENTS_ADD_A_GRADIENT));
        } else {
            // Masks exist, none is selected — one click on the selected
            // row (it toggles) lands here, and the section then showed a
            // list with no adjustments under it and no reason why.
            ui.add_space(SPACE_XS);
            ui.label(
                Text::new(tr(lang, K_DEVELOP_SELECT_A_MASK_ABOVE_TO_EDIT_ITS_ADJUSTMENTS))
                    .weak()
                    .small(),
            );
        }
        changed
    }
    /// Lightroom's Add / Subtract / Intersect grammar: add or drop one shape on
    /// this mask and pick which shape the on-image knobs edit.
    pub(crate) fn dev_mask_shapes(&mut self, ui: &mut egui::Ui, i: usize) -> bool {
        let lang = self.lang;
        let mut changed = false;
        // --- Shapes（组件）: compose extra geometry onto this mask —
        // Lightroom's Add / Subtract / Intersect grammar. Every native
        // shape projects through the same composition spelling; only
        // Bitmap components remain a named geometry loss.
        {
            ui.horizontal(|ui| {
                ui.label(tr(lang, K_DEVELOP_SHAPES));
                let mode_label = |m: autoshade::recipe::MaskCombine| match m {
                    autoshade::recipe::MaskCombine::Add => tr(lang, K_DEVELOP_ADD),
                    autoshade::recipe::MaskCombine::Subtract => tr(lang, K_DEVELOP_SUBTRACT),
                    autoshade::recipe::MaskCombine::Intersect => tr(lang, K_DEVELOP_INTERSECT),
                };
                egui::ComboBox::from_id_salt("comp_mode")
                    .selected_text(mode_label(self.workspace.masks().component_mode))
                    .width(96.0)
                    .show_ui(ui, |ui| {
                        for m in [
                            autoshade::recipe::MaskCombine::Add,
                            autoshade::recipe::MaskCombine::Subtract,
                            autoshade::recipe::MaskCombine::Intersect,
                        ] {
                            ui.selectable_value(&mut self.workspace.masks_mut().component_mode, m, mode_label(m));
                        }
                    });
                let mode = self.workspace.masks().component_mode;
                let lin_armed = matches!(
                    self.workspace.masks().placing_mask,
                    Some((MaskKind::Linear, PlaceTarget::Component(j, _))) if j == i
                );
                if ui
                    .selectable_label(lin_armed, tr(lang, K_DEVELOP_LINEAR))
                    .on_hover_text(tr(lang, K_DEVELOP_DRAG_ON_THE_IMAGE_TO_ADD_A_LINEAR_SHAPE_TO_THIS))
                    .clicked()
                {
                    self.disarm_tools();
                    if !lin_armed {
                        self.workspace.masks_mut().placing_mask =
                            Some((MaskKind::Linear, PlaceTarget::Component(i, mode)));
                        self.workspace.set_status(tr(lang, K_DEVELOP_DRAG_ON_THE_IMAGE_THE_NEW_SHAPE_COMPOSES_ONTO_TH).into());
                    }
                }
                let rad_armed = matches!(
                    self.workspace.masks().placing_mask,
                    Some((MaskKind::Radial, PlaceTarget::Component(j, _))) if j == i
                );
                if ui
                    .selectable_label(rad_armed, tr(lang, K_DEVELOP_RADIAL))
                    .on_hover_text(tr(lang, K_DEVELOP_DRAG_ON_THE_IMAGE_TO_ADD_AN_ELLIPTICAL_SHAPE_TO))
                    .clicked()
                {
                    self.disarm_tools();
                    if !rad_armed {
                        self.workspace.masks_mut().placing_mask =
                            Some((MaskKind::Radial, PlaceTarget::Component(i, mode)));
                        self.workspace.set_status(tr(lang, K_DEVELOP_DRAG_ON_THE_IMAGE_THE_NEW_SHAPE_COMPOSES_ONTO_TH).into());
                    }
                }
            });
            let n_comp = self.editor.doc().recipe.masks[i].components.len();
            let mut del_comp: Option<usize> = None;
            for c in 0..n_comp {
                ui.horizontal(|ui| {
                    let comp = &self.editor.doc().recipe.masks[i].components[c];
                    let icon = match comp.mode {
                        autoshade::recipe::MaskCombine::Add => "＋",
                        autoshade::recipe::MaskCombine::Subtract => "－",
                        autoshade::recipe::MaskCombine::Intersect => "∩",
                    };
                    let kind = match comp.geometry {
                        MaskGeometry::Linear { .. } => tr(lang, K_DEVELOP_LINEAR),
                        MaskGeometry::Shaped { .. } => tr(lang, K_DEVELOP_SHAPED_GRADIENT),
                        MaskGeometry::Radial { .. } => tr(lang, K_DEVELOP_RADIAL),
                        MaskGeometry::Bitmap { .. } => tr(lang, K_DEVELOP_BITMAP),
                        MaskGeometry::Brush { .. } => tr(lang, K_DEVELOP_BRUSH),
                        MaskGeometry::AiMask { .. } => tr(lang, K_DEVELOP_AI),
                    };
                    let selected = self.workspace.masks().sel_component == Some(c);
                    if ui
                        .selectable_label(selected, format!("{icon} {kind}"))
                        .on_hover_text(tr(lang, K_DEVELOP_SELECT_TO_DRAG_THIS_SHAPE_S_KNOBS_ON_THE_IMAGE_T))
                        .clicked()
                    {
                        self.workspace.masks_mut().sel_component = if selected { None } else { Some(c) };
                    }
                    let mut mode = comp.mode;
                    egui::ComboBox::from_id_salt(("comp_row_mode", c))
                        .selected_text(match mode {
                            autoshade::recipe::MaskCombine::Add => tr(lang, K_DEVELOP_ADD__2),
                            autoshade::recipe::MaskCombine::Subtract => tr(lang, K_DEVELOP_SUBTRACT__2),
                            autoshade::recipe::MaskCombine::Intersect => tr(lang, K_DEVELOP_INTERSECT__2),
                        })
                        .width(88.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut mode, autoshade::recipe::MaskCombine::Add, tr(lang, K_DEVELOP_ADD__2));
                            ui.selectable_value(&mut mode, autoshade::recipe::MaskCombine::Subtract, tr(lang, K_DEVELOP_SUBTRACT__2));
                            ui.selectable_value(&mut mode, autoshade::recipe::MaskCombine::Intersect, tr(lang, K_DEVELOP_INTERSECT__2));
                        });
                    if mode != comp.mode {
                        crate::logic::command::apply(
                            &mut self.editor.doc_mut().recipe,
                            crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::SetComponentMode {
                                index: i,
                                component: c,
                                mode,
                            }),
                        );
                        self.workspace.overlay_mut().overlay_stale = true;
                        changed = true;
                    }
                    let mut inverted = self.editor.doc().recipe.masks[i].components[c].inverted;
                    if ui.checkbox(&mut inverted, tr(lang, K_DEVELOP_INVERT)).changed() {
                        crate::logic::command::apply(
                            &mut self.editor.doc_mut().recipe,
                            crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::SetComponentInverted {
                                index: i,
                                component: c,
                                inverted,
                            }),
                        );
                        self.workspace.overlay_mut().overlay_stale = true;
                        changed = true;
                    }
                    if glyph(ui, true, "🗑").clicked() {
                        del_comp = Some(c);
                    }
                });
            }
            if let Some(c) = del_comp {
                crate::logic::command::apply(
                    &mut self.editor.doc_mut().recipe,
                    crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::RemoveComponent {
                        index: i,
                        component: c,
                    }),
                );
                self.workspace.masks_mut().sel_component = match self.workspace.masks().sel_component {
                    Some(s) if s == c => None,
                    Some(s) if s > c => Some(s - 1),
                    other => other,
                };
                self.workspace.overlay_mut().overlay_stale = true;
                changed = true;
            }
            if n_comp > 0 {
                explainer(ui, lang, tr(lang, K_DEVELOP_SHAPES_COMPOSE_IN_ORDER_ONTO_THE_BASE_MASK_LINEA));
            }
        }
        changed
    }

    /// The range editor (luminance or colour) and the Amount every range is
    /// judged at — the last coverage-moving control before the adjustments.
    pub(crate) fn dev_mask_range_and_amount(&mut self, ui: &mut egui::Ui, i: usize) -> bool {
        let lang = self.lang;
        let mut changed = false;
        // --- Range Mask（LR 范围蒙版）: refines WHERE the geometry applies —
        // final weight = geometry × range, live in preview + export + XMP.
        // Coverage invalidation happens ONCE below (after Amount): any
        // edit up to there can move the wash, and the overlay key
        // compare dedupes rebuilds — the old "range-section-only"
        // tracker went blind whenever an earlier control had already
        // set `changed` in the same frame.
        {
            let cur = match &self.editor.doc().recipe.masks[i].range {
                None => 0usize,
                Some(RangeMask::Luminance { .. }) => 1,
                Some(RangeMask::Color { .. }) => 2,
            };
            let mut sel = cur;
            ui.horizontal(|ui| {
                ui.label(tr(lang, K_DEVELOP_RANGE_MASK));
                egui::ComboBox::from_id_salt("range_kind")
                    .selected_text([tr(lang, K_DEVELOP_NONE), tr(lang, K_DEVELOP_LUMINANCE), tr(lang, K_DEVELOP_COLOR)][sel])
                    .width(70.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut sel, 0, tr(lang, K_DEVELOP_NONE));
                        ui.selectable_value(&mut sel, 1, tr(lang, K_DEVELOP_LUMINANCE));
                        ui.selectable_value(&mut sel, 2, tr(lang, K_DEVELOP_COLOR));
                    });
            });
            if sel != cur {
                // Leaving Color: the armed sampler goes too, or the
                // canvas keeps advertising (and dispatching) a colour
                // pick for a range that no longer exists.
                if cur == 2 && self.workspace.tools().range_picking == Some(i) {
                    self.workspace.tools_mut().range_picking = None;
                }
                let range = match sel {
                    // Full range = neutral start; narrow from there.
                    1 => Some(RangeMask::Luminance { lo_outer: 0.0, lo: 0.0, hi: 1.0, hi_outer: 1.0 }),
                    2 => Some(RangeMask::Color { r: 0.5, g: 0.5, b: 0.5, amount: 0.5, px: 0.5, py: 0.5 }),
                    _ => None,
                };
                crate::logic::command::apply(
                    &mut self.editor.doc_mut().recipe,
                    crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::SetRange { index: i, range }),
                );
                if sel == 2 {
                    // Jump straight into sampling — a colour range without
                    // a picked colour selects nothing useful.
                    self.disarm_tools();
                    self.workspace.tools_mut().range_picking = Some(i);
                    self.workspace.set_status(tr(lang, K_DEVELOP_COLOR_RANGE_CLICK_THE_COLOR_TO_PICK_IN_THE_IMAGE).into());
                }
                changed = true;
            }
            let picking_this = self.workspace.tools().range_picking == Some(i);
            let mut want_pick = false;
            match &mut self.editor.doc_mut().recipe.masks[i].range {
                Some(RangeMask::Luminance { lo_outer, lo, hi, hi_outer }) => {
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, K_DEVELOP_ORDERED_BOUNDS));
                        ui.monospace(format!(
                            "{lo_outer:.3} <= {lo:.3} <= {hi:.3} <= {hi_outer:.3}"
                        ));
                    });
                    // GUI shows lo/hi + one symmetric feather; the recipe keeps
                    // ACR's 4-number trapezoid (asymmetric AI trapezoids show
                    // their averaged feather until a slider is touched).
                    // All three on LR's 0..100 display track (storage
                    // stays the 0..1 fraction — see slider_pct).
                    let mut f = ((*lo - *lo_outer) + (*hi_outer - *hi)) * 0.5;
                    let ch_lo = slider_pct(ui, lang, tr(lang, K_DEVELOP_LUM_LOW), lo, 1.0, 0.0);
                    let ch_hi = slider_pct(ui, lang, tr(lang, K_DEVELOP_LUM_HIGH), hi, 1.0, 1.0);
                    let ch_f = slider_pct(ui, lang, tr(lang, K_DEVELOP_FEATHER), &mut f, 0.5, 0.1);
                    if ch_lo || ch_hi || ch_f {
                        // CLAMP the edited endpoint at the other, never
                        // swap: the active slider stays bound to its
                        // field across drag frames, so swapping made
                        // both bounds follow the pointer and converge.
                        if *lo > *hi {
                            if ch_lo { *lo = *hi } else { *hi = *lo }
                        }
                        *lo_outer = (*lo - f).max(0.0);
                        *hi_outer = (*hi + f).min(1.0);
                        changed = true;
                    }
                }
                Some(RangeMask::Color { r, g, b, amount, .. }) => {
                    ui.horizontal(|ui| {
                        let full = columns(ui, 1);
                        let mut c = [*r, *g, *b];
                        let swatch = ui.color_edit_button_rgb(&mut c);
                        if swatch.changed() {
                            [*r, *g, *b] = [c[0], c[1], c[2]];
                            changed = true;
                        }
                        // The verb fills the rest of the row after the
                        // swatch (2026-09-27), as the gallery's ✕ row does.
                        let cell = egui::vec2(
                            (full.x - swatch.rect.width() - ui.spacing().item_spacing.x).max(0.0),
                            full.y,
                        );
                        let label = if picking_this { tr(lang, K_DEVELOP_CLICK_IN_IMAGE) } else { tr(lang, K_DEVELOP_SAMPLE) };
                        if action_in(ui, cell, true, label).on_hover_text(tr(lang, K_DEVELOP_CLICK_THE_COLOR_TO_PICK_IN_THE_IMAGE_THE_SAME_CO)).clicked() {
                            want_pick = true;
                        }
                    });
                    changed |= slider_pct(ui, lang, tr(lang, K_DEVELOP_TOLERANCE), amount, 1.0, 0.5);
                }
                None => {}
            }
            if want_pick {
                self.disarm_tools();
                if !picking_this {
                    self.workspace.tools_mut().range_picking = Some(i);
                    self.workspace.set_status(tr(lang, K_DEVELOP_COLOR_RANGE_CLICK_THE_COLOR_TO_PICK_IN_THE_IMAGE).into());
                }
            }
        }
        let m = &mut self.editor.doc_mut().recipe.masks[i];
        // LR's 0..100 display track (storage stays 0..1 — slider_pct).
        changed |= slider_pct(ui, lang, tr(lang, K_DEVELOP_AMOUNT), &mut m.amount, 1.0, 1.0);
        changed
    }

    /// The name/verb row, the radial knobs and the raster tools: the edits that
    /// belong to the mask itself rather than to its composition.
    pub(crate) fn dev_mask_verbs_and_geometry(&mut self, ui: &mut egui::Ui, i: usize) -> bool {
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_MD);
        ui.horizontal(|ui| {
            // The name edits a LOCAL buffer and commits one recipe
            // mutation on focus loss (Enter included) — binding the
            // TextEdit straight to the recipe pushed one undo step per
            // keystroke, so "sky gradient" burned 12 history slots and
            // Ctrl+Z walked the name back letter by letter. When the
            // selection jumps to another row mid-edit, the pending
            // rename commits to ITS OWN mask first — but only while
            // that mask's name still equals the seed-time snapshot, so
            // index shifts (delete/reorder) can never cross-commit.
            if self.workspace.masks().mask_name_buf.as_ref().is_none_or(|(j, ..)| *j != i) {
                if let Some((j, orig, buf)) = self.workspace.masks_mut().mask_name_buf.take() {
                    let same = self.editor.doc().recipe.masks.get(j).is_some_and(|prev| prev.name == orig);
                    if same && buf != orig {
                        crate::logic::command::apply(
                            &mut self.editor.doc_mut().recipe,
                            crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::Rename { index: j, name: buf }),
                        );
                        changed = true;
                    }
                }
                let cur = self.editor.doc().recipe.masks[i].name.clone();
                self.workspace.masks_mut().mask_name_buf = Some((i, cur.clone(), cur));
            }
            // R38: the name takes what the row leaves after the verbs
            // beside it (↻ Redraw when the geometry has one, then
            // ⬆ ⬇ ⧉), so the row ends where the grid rows below it
            // end. At a fixed 110 px, with the two switches also on
            // this row, it ran 100 px past the panel — and the panel
            // widened to fit, taking every row below it along.
            let has_redraw = matches!(
                self.editor.doc().recipe.masks[i].mask,
                MaskGeometry::Linear { .. } | MaskGeometry::Radial { .. }
            );
            let reserve = {
                let sp = ui.spacing();
                let redraw = if has_redraw {
                    let label = ui.fonts_mut(|f| {
                        f.layout_no_wrap(
                            tr(lang, K_DEVELOP_REDRAW).to_owned(),
                            egui::TextStyle::Button.resolve(ui.style()),
                            egui::Color32::WHITE,
                        )
                        .size()
                        .x
                    });
                    label + 2.0 * sp.button_padding.x + sp.item_spacing.x
                } else {
                    0.0
                };
                redraw + 3.0 * (crate::buttons::row_h(ui) + sp.item_spacing.x)
            };
            // The frame's own margin (egui's 4 px each side) lies
            // outside `desired_width`, so it comes off the budget too.
            let name_margin = egui::Margin::symmetric(4, 2);
            let name_resp = {
                let buf = &mut self.workspace.masks_mut().mask_name_buf.as_mut().expect("seeded above").2;
                ui.add(
                    egui::TextEdit::singleline(buf)
                        .margin(name_margin)
                        .desired_width(
                            (ui.available_width() - reserve - name_margin.sum().x).max(110.0),
                        )
                        .hint_text(tr(lang, K_DEVELOP_NAME)),
                )
            };
            if name_resp.lost_focus()
                && let Some((_, orig, buf)) = self.workspace.masks_mut().mask_name_buf.as_mut()
                && *buf != self.editor.doc().recipe.masks[i].name
            {
                crate::logic::command::apply(
                    &mut self.editor.doc_mut().recipe,
                    crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::Rename {
                        index: i,
                        name: buf.clone(),
                    }),
                );
                // Re-baseline the seed: a SECOND rename in this row
                // must still cross-commit correctly on a row switch.
                *orig = buf.clone();
                changed = true;
            }
            let m = &mut self.editor.doc_mut().recipe.masks[i];
            // Raster masks have no drag-to-place geometry — no 重画.
            let kind = match m.mask {
                MaskGeometry::Linear { .. } => Some(MaskKind::Linear),
                MaskGeometry::Radial { .. } => Some(MaskKind::Radial),
                // Neither a raster nor a brush group has drag-to-place
                // geometry — no 重画 for either.
                // An AI mask has no drag-to-place geometry either:
                // its reference point is a MODEL PROMPT, so "redraw"
                // would mean re-running the segmenter, not moving a
                // shape — a different action from this button's.
                // A shaped gradient's curve was measured from a
                // target; redrawing its handles would keep a curve
                // measured for another span.
                MaskGeometry::Bitmap { .. }
                | MaskGeometry::Brush { .. }
                | MaskGeometry::AiMask { .. }
                | MaskGeometry::Shaped { .. } => None,
            };
            if let Some(kind) = kind {
                let redraw_armed = matches!(
                    self.workspace.masks().placing_mask,
                    Some((k, PlaceTarget::Redraw(j))) if k == kind && j == i
                );
                if ui
                    .selectable_label(redraw_armed, tr(lang, K_DEVELOP_REDRAW))
                    .on_hover_text(tr(lang, K_DEVELOP_RE_DRAG_THIS_MASK_S_AREA_ON_THE_IMAGE))
                    .clicked()
                {
                    self.disarm_tools();
                    if !redraw_armed {
                        self.workspace.masks_mut().placing_mask = Some((kind, PlaceTarget::Redraw(i)));
                        self.workspace.set_status(tr(lang, K_DEVELOP_RE_DRAG_THIS_MASK_S_AREA_ON_THE_IMAGE).into());
                    }
                }
            }
            // Mask ORDER is render semantics (masks stack sequentially;
            // a later mask's range sees earlier masks' output) — so the
            // list order is editable, not just cosmetic.
            if glyph(ui, i > 0, "⬆")
                .on_hover_text(tr(lang, K_DEVELOP_MOVE_UP_RENDERS_EARLIER))
                .clicked()
            {
                self.act_all(vec![logic::Action::Develop(
                    logic::develop::DevelopAction::MoveMaskUp(i),
                )]);
            }
            if glyph(ui, i + 1 < self.editor.doc().recipe.masks.len(), "⬇")
                .on_hover_text(tr(lang, K_DEVELOP_MOVE_DOWN_RENDERS_LATER))
                .clicked()
            {
                self.act_all(vec![logic::Action::Develop(
                    logic::develop::DevelopAction::MoveMaskDown(i),
                )]);
            }
            // Duplicate: a second gradient with the same tuned ten
            // sliders; rasters are DETACHED copies (the version-load
            // rule), so the twins never share a mutable file.
            if glyph(ui, true, "⧉")
                .on_hover_text(tr(lang, K_DEVELOP_DUPLICATE_THIS_MASK_BITMAP_RASTERS_ARE_COPIED_SO))
                .clicked()
            {
                self.act_all(vec![logic::Action::Develop(
                    logic::develop::DevelopAction::DuplicateMask(i),
                )]);
            }
        });
        // R38: the two switches on their own row, under the verbs.
        ui.horizontal(|ui| {
            // Inversion flips the mask's coverage — its Response.changed()
            // must drive the develop + overlay like every other mask
            // control (was silently discarded: the toggle mutated the
            // recipe but never re-rendered until an unrelated edit).
            if ui.checkbox(&mut self.editor.doc_mut().recipe.masks[i].inverted, tr(lang, K_DEVELOP_INVERT)).changed() {
                self.workspace.overlay_mut().overlay_stale = true;
                changed = true;
            }
            // ONE view switch, not a property of this row (R22 #16): it
            // is the same flag the O key toggles, and
            // `refresh_mask_overlay` (canvas.rs) always draws whichever
            // mask is hovered-or-selected. Sitting in the selected-mask
            // row is right — that IS the mask it draws — but 「Overlay」
            // beside 「Invert」 (and, before R38, 「↻ Redraw」 and the
            // ⬆/⬇ order buttons) reads as
            // per-mask state that would be remembered per mask. The
            // label now says what it is, in the same words the F1 sheet
            // uses for the O key ("Toggle mask overlay").
            if ui
                .checkbox(&mut self.workspace.overlay_mut().show_mask_overlay, tr(lang, K_DEVELOP_SHOW_MASK_OVERLAY))
                .on_hover_text(tr(lang, K_DEVELOP_ONE_VIEW_SWITCH_SHARED_BY_EVERY_MASK_SHORTCUT_O))
                .changed()
            {
                self.workspace.overlay_mut().overlay_stale = true;
            }
        });
        // Radial geometry: edge feather (shown 0..100, LR's track) +
        // rotation + inside/outside flip — all engine-rendered.
        if let MaskGeometry::Radial { feather, flipped, angle, .. } =
            &mut self.editor.doc_mut().recipe.masks[i].mask
        {
            let mut geo_ch = false;
            ui.horizontal(|ui| {
                geo_ch |= slider_pct(
                    ui,
                    lang,
                    tr(lang, K_DEVELOP_EDGE_FEATHER),
                    feather,
                    1.0,
                    0.5,
                );
                geo_ch |= ui
                    .checkbox(flipped, tr(lang, K_DEVELOP_FLIP))
                    .on_hover_text(tr(
                        lang,
                        K_DEVELOP_SWAP_WHICH_SIDE_OF_THE_ELLIPSE_THE_ADJUSTMENT_AF,
                    ))
                    .changed();
            });
            // Rotation: the on-image grip (the knob parked above the
            // ellipse) drags it too; the slider gives numeric entry.
            // -179.9: clamp_geometry normalizes to (-180, 180], so a
            // drag to the very left snapped the knob to +180 — the
            // ellipse never moved, the knob teleported (L15-6; the
            // hue-slider rule).
            geo_ch |= slider(
                ui,
                lang,
                tr(lang, K_DEVELOP_ANGLE),
                angle,
                -179.9,
                180.0,
                0.0,
            );
            if geo_ch {
                self.workspace.overlay_mut().overlay_stale = true;
                changed = true;
            }
        }
        // Raster tools: before these, a raster mask was completely
        // un-editable after creation — a clipped treeline's only
        // recourse was re-running the model to get the same raster.
        // Every op BAKES a freshly claimed raster and repoints (never
        // mutates the input file).
        //
        // The gate is "does this mask have PIXELS", not "is it a
        // `Bitmap`": the reverse-fit zones are Select Sky components
        // now and still ride an ordinary claimed PNG, while an AI mask
        // whose alpha has not resolved has nothing to open.
        if autoshade::render::geometry_raster_path(&self.editor.doc().recipe.masks[i].mask).is_some() {
            // R38: two aligned rows — the pair that opens a session
            // or a full-size re-cut, then the three one-step
            // morphologies — instead of five free-width buttons
            // wrapping wherever the panel width fell.
            ui.horizontal(|ui| {
                let cell = columns(ui, 2);
                let edit_armed = matches!(self.workspace.masks().mask_brush, Some((Some(j), _)) if j == i);
                if toggle_in(ui, cell, edit_armed, tr(lang, K_DEVELOP_EDIT_RASTER))
                    .on_hover_text(tr(lang, K_DEVELOP_BRUSH_EDIT_THIS_MASK_PAINT_ADDS_ERASE_REMOVES_AP))
                    .clicked()
                {
                    if edit_armed {
                        self.disarm_tools();
                    } else {
                        self.act_all(vec![logic::Action::Develop(
                        logic::develop::DevelopAction::MaskBrush(Some(i)),
                    )]);
                    }
                }
                if action_in(ui, cell, !self.workspace.busy(), tr(lang, K_DEVELOP_FULL_RES_REFINE))
                    .on_hover_text(tr(lang, K_DEVELOP_RE_CUT_THIS_MASK_AGAINST_THE_FULL_RESOLUTION_SOU))
                    .clicked()
                {
                    self.act_all(vec![logic::Action::Develop(
                    logic::develop::DevelopAction::MaskRefine(i),
                )]);
                }
            });
            ui.horizontal(|ui| {
                let cell = columns(ui, 3);
                if action_in(ui, cell, true, tr(lang, K_DEVELOP_SOFTEN))
                    .on_hover_text(tr(lang, K_DEVELOP_SOFTEN_THE_MASK_BOUNDARY_ONE_STEP_BAKES_A_NEW_RA))
                    .clicked()
                {
                    crate::masks::bake_mask_raster(self.editor, self.workspace, self.lang, 
                        i,
                        |g| {
                            let s = (g.width().max(g.height()) as f32 / 400.0).max(1.0);
                            autoshade::render::feather_mask(g, s)
                        },
                        "mask-edit",
                    );
                }
                if action_in(ui, cell, true, tr(lang, K_DEVELOP_EXPAND))
                    .on_hover_text(tr(lang, K_DEVELOP_GROW_THE_SELECTION_ONE_STEP_BAKES_A_NEW_RASTER))
                    .clicked()
                {
                    crate::masks::bake_mask_raster(self.editor, self.workspace, self.lang, 
                        i,
                        |g| {
                            let r = (g.width().max(g.height()) / 500).max(2) as i32;
                            autoshade::render::morph_mask(g, r)
                        },
                        "mask-edit",
                    );
                }
                if action_in(ui, cell, true, tr(lang, K_DEVELOP_CONTRACT))
                    .on_hover_text(tr(lang, K_DEVELOP_SHRINK_THE_SELECTION_ONE_STEP_BAKES_A_NEW_RASTER))
                    .clicked()
                {
                    crate::masks::bake_mask_raster(self.editor, self.workspace, self.lang, 
                        i,
                        |g| {
                            let r = (g.width().max(g.height()) / 500).max(2) as i32;
                            autoshade::render::morph_mask(g, -r)
                        },
                        "mask-edit",
                    );
                }
            });
        }
        changed
    }

    /// The AI segmentation pair and the empty-list line. `n_masks` is the count
    /// the caller already read for the section title.
    pub(crate) fn dev_mask_ai_verbs(&mut self, ui: &mut egui::Ui, n_masks: usize) {
        let lang = self.lang;
        // --- AI segmentation → bitmap masks (gap batch A②) ---------------
        ui.horizontal(|ui| {
            // CAPABILITY, not just state: without the python sidecar on
            // disk these two can only fail, so the button says so up front
            // instead of spending the click on a "not found at …" toast.
            let has_helper = segment_helper_available();
            let can_seg = !self.workspace.busy() && self.editor.photo().base_preview.is_some() && has_helper;
            let cell = columns(ui, 2);
            let missing = tr(lang,
                K_DEVELOP_THIS_FEATURE_NEEDS_AUTOSHADE_S_PYTHON_HELPER_WHI,
            );
            // 🤖 on BOTH (#4): one prefix marks every AI verb app-wide, and
            // 「☁ AI select sky」 was the lone exception — a weather glyph
            // where its twin one row over already wore the robot. The
            // tooltip carries the AI-panel cross-reference, but only on the
            // arm that CAN run: a missing-sidecar message must stay about
            // the missing sidecar.
            if action_in(ui, cell, can_seg, tr(lang, K_DEVELOP_SELECT_SUBJECT))
                // R29 B4: the model, its dependencies and its download size
                // all changed with the user's ruling of 2026-08-21, and the
                // FALLBACK is named because a machine without torchvision
                // gets a materially softer mask and would otherwise have no
                // way to know which model drew it.
                .on_hover_text(if has_helper { ai_xref(lang, tr(lang,
                    K_DEVELOP_BIREFNET_SALIENT_SUBJECT_SEGMENTATION_BITMAP_MAS,
                )) } else { missing.to_string() })
                .clicked()
            {
                self.act_all(vec![logic::Action::Develop(
                    logic::develop::DevelopAction::Segment { target: "subject", label: "Subject" },
                )]);
            }
            if action_in(ui, cell, can_seg, tr(lang, K_DEVELOP_SELECT_SKY))
                .on_hover_text(if has_helper { ai_xref(lang, tr(lang,
                    K_DEVELOP_ONEFORMER_ADE20K_SKY_SEGMENTATION_BITMAP_MASK_PY,
                )) } else { missing.to_string() })
                .clicked()
            {
                self.act_all(vec![logic::Action::Develop(
                    logic::develop::DevelopAction::Segment { target: "sky", label: "Sky" },
                )]);
            }
        });
        // Empty state: an open section with zero rows looked like a bug;
        // one quiet line says where masks come from.
        if n_masks == 0 {
            ui.add_space(SPACE_XS);
            ui.label(
                Text::new(tr(
                    lang,
                    K_DEVELOP_NO_MASKS_YET_DRAW_ONE_WITH_THE_TOOLS_ABOVE_AI_AN,
                ))
                .weak()
                .small(),
            );
        }
    }
    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_export(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        let lang = self.lang;
    
        // --- 导出设置 (UX batch): moved out of the toolbar — touched once per
        // delivery, these are Export-dialog contents, not toolbar chrome. The
        // toolbar keeps the ACTIONS; their hover echoes this section's state.
        ui.add_space(SPACE_MD);
        // Delivery preferences have no per-photo neutral, so this header has
        // no adjustment dot. The toolbar's Export hover carries the summary.
        section_fold(ui, "sec_export", tr(lang, K_APP_EXPORT), false, false, |ui| {
                // WHERE first (R22-7): the delivery target is now a setting
                // rather than a property of which toolbar button was pressed,
                // and it is the one whose wrong value sends the file somewhere
                // the user has to hunt for — so it leads the section instead of
                // hiding under the encoder dials.
                ui.horizontal(|ui| {
                    ui.label(tr(lang, K_DEVELOP_DESTINATION));
                    egui::ComboBox::from_id_salt("exp_dest")
                        .selected_text(tr(lang, self.workspace.exp().dest.label()))
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for d in ExportDest::ALL {
                                ui.selectable_value(&mut self.workspace.exp_mut().dest, d, tr(lang, d.label()));
                            }
                        });
                });
                // The RESOLVED target, spelled out: "./out" is relative to
                // whichever directory the app was launched from, and "last used
                // folder" says nothing about which folder that is.
                ui.label(
                    Text::new(match self.workspace.export_dest_dir() {
                        Some(d) => abs_display(&d),
                        None => tr(lang, K_DEVELOP_A_SAVE_DIALOG_OPENS_ON_EVERY_EXPORT).to_string(),
                    })
                    .weak()
                    .small(),
                );
                // One setting per row: two label+combo pairs in a non-wrapping
                // horizontal overflowed the 320px panel in Chinese.
                ui.horizontal(|ui| {
                    ui.label(tr(lang, K_DEVELOP_FORMAT));
                    egui::ComboBox::from_id_salt("save_fmt")
                        .selected_text(tr(lang, self.workspace.exp().format.label()))
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            for f in ExportFormat::ALL {
                                ui.selectable_value(&mut self.workspace.exp_mut().format, f, tr(lang, f.label()));
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label(tr(lang, K_DEVELOP_LONG_EDGE));
                    egui::ComboBox::from_id_salt("exp_long_edge")
                        .selected_text(if self.workspace.exp().long_edge == 0 {
                            tr(lang, K_EXPORT_ORIGINAL_SIZE).to_string()
                        } else {
                            format!("{} px", self.workspace.exp().long_edge)
                        })
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.workspace.exp_mut().long_edge, 0, tr(lang, K_EXPORT_ORIGINAL_SIZE));
                            for px in [1600u32, 2048, 2560, 3840, 5120] {
                                ui.selectable_value(&mut self.workspace.exp_mut().long_edge, px, format!("{px} px"));
                            }
                        });
                });
                slider(ui, lang, tr(lang, K_EXPORT_OUTPUT_SHARPENING), &mut self.workspace.exp_mut().sharpen, 0.0, 100.0, 0.0);
                // Always allocated, merely disabled for TIFF — the old
                // appear/disappear reflowed every control to its right.
                ui.add_enabled_ui(self.workspace.exp().format == ExportFormat::Jpeg, |ui| {
                    slider(ui, lang, tr(lang, K_DEVELOP_JPEG_QUALITY), &mut self.workspace.exp_mut().quality, 60.0, 100.0, 95.0);
                });
                ui.horizontal(|ui| {
                    ui.label(tr(lang, K_DEVELOP_COLOUR_SPACE));
                    egui::ComboBox::from_id_salt("exp_space")
                        .selected_text(tr(lang, EXPORT_SPACES[(self.workspace.exp().space as usize).min(2)]))
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for (i, name) in EXPORT_SPACES.iter().enumerate() {
                                ui.selectable_value(&mut self.workspace.exp_mut().space, i as u8, tr(lang, name));
                            }
                        });
                });
                // 「on export」 completes the pair the 🤖 prefix alone could not
                // separate (R22 #16): this checkbox and the Detail section's
                // 「🤖 AI Denoise now」 are two TIMINGS of one denoiser — this one
                // runs inside every full-resolution delivery and touches no
                // pixel until then, that one bakes a clean base into the current
                // variant immediately. Same-named twins in two panels made the
                // difference invisible until an export took minutes.
                ui.checkbox(&mut self.workspace.exp_mut().save_denoise, tr(lang, K_DEVELOP_AI_DENOISE_ON_EXPORT)).on_hover_text(
                    ai_xref(lang, tr(lang, K_DEVELOP_AI_DENOISE_BEFORE_DEVELOPING_AT_THE_EXPORT_DENOI)),
                );
                // The checkbox's OWN dial, directly under it (user decision
                // 2026-09-12): the Detail fold's 「AI Denoise now」 has its own
                // and neither reaches the other. Always allocated, merely
                // disabled while the checkbox is off — the appear/disappear
                // reflow rule the JPEG quality slider above follows.
                let denoise_raw = self.editor.denoise_uses_raw();
                ui.add_enabled_ui(self.workspace.exp().save_denoise, |ui| {
                    slider_pct_hinted(
                        ui,
                        lang,
                        tr(lang, K_DEVELOP_EXPORT_DENOISE_STRENGTH),
                        if denoise_raw { &mut self.workspace.exp_mut().save_denoise_strength } else { &mut self.workspace.exp_mut().baked_save_denoise_strength },
                        1.0,
                        if denoise_raw { autoshade::denoise::DEFAULT_STRENGTH_RAW } else { autoshade::denoise::DEFAULT_STRENGTH },
                        tr(lang,
                            K_DEVELOP_EXPORT_AI_DENOISE_STRENGTH_INDEPENDENT_OF_THE_DE,
                        ),
                    );
                });
                explainer(ui, lang, tr(lang, K_DEVELOP_APPLIED_BY_EXPORT_IN_THE_TOOLBAR_CTRL_SHIFT_E_OR));
                // --- SF8-A: hand the Lightroom sidecar over ----------------
                // The XMP projection has always been written INSIDE the hashed
                // develop folder, which is the right home for state — but the
                // one thing users do with an XMP is give it to Lightroom, and
                // Lightroom only looks for it beside the photo. That left the
                // hand-off as "browse into %LOCALAPPDATA% yourself".
                ui.add_space(SPACE_LG);
                let raw = self.editor.can_export_xmp_beside();
                let label = if self.workspace.exp().xmp_beside_confirm {
                    tr(lang, K_DEVELOP_OVERWRITE_THE_XMP_ALREADY_THERE)
                } else {
                    tr(lang, K_DEVELOP_EXPORT_XMP_BESIDE_THE_PHOTO)
                };
                let hint = if !raw {
                    // The disabled REASON, not a repeat of the label: only a
                    // camera RAW has the sidecar convention (store::
                    // lightroom_sidecar and pipeline::write_xmp draw the same
                    // line), so a baked PNG/TIFF's neighbouring .xmp belongs to
                    // some other program and must not be written over.
                    tr(lang, K_DEVELOP_RAW_ONLY_A_BAKED_PNG_TIFF_HAS_NO_LIGHTROOM_SIDEC)
                } else if self.workspace.exp().xmp_beside_confirm {
                    tr(lang, K_DEVELOP_A_XMP_ALREADY_SITS_BESIDE_THIS_PHOTO_LIGHTROOM_S)
                } else {
                    tr(lang, K_DEVELOP_COPY_THIS_PHOTO_S_STORED_LIGHTROOM_ACR_SIDECAR_I)
                };
                if action_row(ui, raw && !self.workspace.busy(), label)
                    .on_hover_text(hint)
                    .clicked()
                {
                    self.editor.export_xmp_beside(self.workspace, self.lang);
                }
            });
        false
    }
    /// One capability table: the renderer draws each control through its own
    /// implementation and reports a data change (id + value); the registry
    /// applies it. The panel does not know a control by name.
    pub(crate) fn dev_tone_wb(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        // DERIVED from the registry like the five section dots in
        // `develop_panel` (R25 P8 closed the last two hand-written ones). This
        // section holds BOTH the tone family and the white-balance one — the
        // panel groups them, the registry does not — so its ● is the OR, the
        // same shape the Detail and Lens sections already use for their own
        // pairs.
        let tone_active = CONTROL_FAMILIES
            .iter()
            .filter(|f| f.name == "tone" || f.name == "white_balance")
            .any(|f| family_is_active(f, &self.editor.doc().recipe));
    
        // #14b: the FIRST of the panel's group captions (its air is its own:
        // `headings::group_caption`). The two existing group
        // fences (before Detail, before Local Masks) marked boundaries without
        // ever saying what they divided; the five groups the panel actually has
        // are AI (its own panel above) → tone & colour → detail & lens → local
        // & pixel → versions & export. Captions only: no section moved, no
        // fence rhythm changed.
        group_caption(ui, tr(lang, K_DEVELOP_TONE_COLOUR));
        section_fold(ui, "sec_tone", section_title(tr(lang, K_DEVELOP_TONE_WB), tone_active), true, false, |ui| {
                // R38: wrapped, so the eyedropper drops under the checkbox when
                // the row is short — the pair is 351 px at the 320 px default,
                // and a plain `horizontal` never wraps: it widened the
                // auto-fitting panel by 64 px, the widest row in this panel.
                ui.horizontal_wrapped(|ui| {
                    // One cell of the grid for the eyedropper, measured at the
                    // row's start (2026-09-27); it wraps under the checkbox.
                    let cell = columns(ui, 2);
                    // A nonzero tint IS a WB edit (recipes saved before the
                    // uncheck-zeroes-tint fix can carry one with no Temp) —
                    // showing "off = as-shot" over an active tint, with its
                    // slider disabled, was a lie with no way out.
                    let mut custom_wb =
                        self.editor.doc().recipe.temperature_k.is_some() || self.editor.doc().recipe.tint != 0.0;
                    if ui.checkbox(&mut custom_wb, tr(lang, K_DEVELOP_CUSTOM_WHITE_BALANCE_OFF_AS_SHOT)).changed() {
                        // Arm AT the as-shot anchor when stamped: checking
                        // the box must not shift the image — the slider then
                        // starts from the camera's real Kelvin, not 5500.
                        self.editor.doc_mut().recipe.temperature_k =
                            if custom_wb { Some(self.editor.doc().recipe.as_shot_k.unwrap_or(5500.0)) } else { None };
                        if !custom_wb {
                            // "As-shot" is the WHOLE white balance: a custom
                            // Tint surviving the un-check kept a WB edit
                            // active behind a label claiming there was none.
                            self.editor.doc_mut().recipe.tint = 0.0;
                        }
                        changed = true;
                    }
                    let label = if self.workspace.tools().wb_picking { tr(lang, K_DEVELOP_CLICK_IN_IMAGE) } else { tr(lang, K_DEVELOP_EYEDROPPER) };
                    if action_in(ui, cell, true, label)
                        .on_hover_text(tr(lang,
                            K_DEVELOP_CLICK_A_SPOT_IN_THE_IMAGE_THAT_SHOULD_BE_NEUTRAL,
                        ))
                        .clicked()
                    {
                        let on = !self.workspace.tools().wb_picking;
                        self.disarm_tools();
                        self.workspace.tools_mut().wb_picking = on;
                        if on {
                            self.workspace.set_status(tr(lang, K_APP_WB_EYEDROPPER_CLICK_A_SPOT_THAT_SHOULD_BE_NEUTRA).into());
                        }
                    }
                });
                if let (Some(k), Some(t)) = (self.editor.doc().recipe.as_shot_k, self.editor.doc().recipe.as_shot_tint) {
                    // The camera's own WB in absolute terms — the reference
                    // the Temp slider is anchored on for this photo. BOTH
                    // halves required: the era pin (anchor 5500, tint None)
                    // knows the anchor but NOT the camera, and showing it as
                    // "as shot" would be a false claim.
                    ui.weak(trf(
                        lang,
                        K_DEVELOP_AS_SHOT_K_TINT,
                        &[("k", &format!("{k:.0}")), ("t", &format!("{t:+.0}"))],
                    ));
                }
                // Double-click reset lands on the as-shot Kelvin when stamped
                // (the honest "no shift" position), 5500 for legacy recipes.
                let anchor_k = self.editor.doc().recipe.as_shot_k.unwrap_or(5500.0);
                if let Some(mut k) = self.editor.doc().recipe.temperature_k
                    && slider_log(ui, lang, tr(lang, K_DEVELOP_TEMP_K), &mut k, 2000.0, 50000.0, anchor_k)
                {
                    self.editor.doc_mut().recipe.temperature_k = Some(k);
                    changed = true;
                }
                let custom_wb = self.editor.doc().recipe.temperature_k.is_some() || self.editor.doc().recipe.tint != 0.0;
                // Tint is HALF of the white balance: editable only under
                // Custom WB, or a tint move silently contradicted the
                // checkbox's "off = as-shot" promise. A legacy tint-only
                // recipe counts as Custom (see the checkbox above) so its
                // slider stays reachable.
                ui.add_enabled_ui(custom_wb, |ui| {
                    changed |= self.capability_rows(ui, crate::controls::WB_TINT);
                });
                // The tone band is a capability table row each: the panel owns
                // the fold and the white balance controls above; the fields,
                // bands and resets live in `logic::capability::TONE`.
                changed |= self.capability_rows(ui, crate::controls::TONE);
            });
        changed
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_crop(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        // --- 裁剪 + 拉直: recipe.crop / straighten_deg (export + XMP paths) ---
        ui.add_space(SPACE_MD);
        // The `framing` family IS this section — crop rectangle plus
        // straighten angle — so the dot is read straight off the registry
        // (R25 P8, the same derivation as every other section's).
        let crop_active = CONTROL_FAMILIES
            .iter()
            .filter(|f| f.name == "framing")
            .any(|f| family_is_active(f, &self.editor.doc().recipe));
        section_fold(ui, "sec_crop", section_title(tr(lang, K_DEVELOP_CROP), crop_active), false, false, |ui| {
                // The two verbs share the row as equal cells and the aspect
                // preset sits under them on a captioned row of its own
                // (2026-09-27): a 70 px combo between two own-width verbs was
                // the one three-item row in this panel.
                ui.horizontal(|ui| {
                    let cell = columns(ui, 2);
                    // ✓ (geometric) — same finish-glyph as 「✓ Apply」; the
                    // emoji ✅ was the odd one out of the check family.
                    let label = if self.workspace.crop().mode { tr(lang, K_DEVELOP_DONE) } else { tr(lang, K_DEVELOP_ENTER_CROP) };
                    if action_in(ui, cell, true, label).clicked() {
                        let on = !self.workspace.crop().mode;
                        self.disarm_tools();
                        self.editor.set_crop_mode(self.workspace, on);
                    }
                    if action_in(ui, cell, true, tr(lang, K_DEVELOP_CLEAR_CROP)).clicked()
                        && self.editor.doc_mut().recipe.crop.take().is_some()
                    {
                        // Through the panel's own change path: clamp + dirty,
                        // so the crop-restricted histogram/clipping refresh.
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(Text::new(tr(lang, K_DEVELOP_ASPECT)).weak().small());
                    let prev_aspect = self.workspace.crop().aspect;
                    egui::ComboBox::from_id_salt("crop_aspect")
                        .selected_text(tr(lang, CROP_ASPECTS[self.workspace.crop().aspect].0))
                        .width(70.0)
                        .show_ui(ui, |ui| {
                            for (i, (name, _)) in CROP_ASPECTS.iter().enumerate() {
                                ui.selectable_value(&mut self.workspace.crop_mut().aspect, i, tr(lang, name));
                            }
                        });
                    // LR applies a picked preset to the box IMMEDIATELY — the
                    // combo used to only arm the ratio for the NEXT handle
                    // drag, so the UI said 4:5 while preview/export kept the
                    // old shape (D13). Consumed in handle_crop, where the
                    // view dims live — and armed ONLY while the crop tool is
                    // up: with the tool away the flag would survive until
                    // some later session's crop entry and rewrite the box as
                    // a surprise (Codex batch-38); tool-off keeps the old
                    // arm-for-the-next-drag meaning.
                    if self.workspace.crop().aspect != prev_aspect && self.workspace.crop().mode {
                        self.workspace.crop_mut().aspect_pending = true;
                    }
                });
                // Straighten: rotate + auto-crop (engine rotate_straighten);
                // the preview shows exactly the export geometry. Fine class:
                // levelling needs 0.1°, not the wide-range integer snap.
                changed |= self.capability_rows(ui, crate::controls::CROP);
                explainer(ui, lang, tr(lang, K_DEVELOP_ONCE_IN_R_DRAG_CORNER_EDGE_HANDLES_TO_RESIZE_DRA));
            });
        changed
    }
    /// Recipe replaced wholesale (undo/redo/Reset/version load/AI apply): the
    /// derived display state must follow, or the panel keeps showing an AI
    /// rationale / verdict that describes a recipe no longer on screen — and
    /// an ARMED index-carrying tool (range sampler, ↻ Redraw) would fire into
    /// whatever mask now happens to sit at its remembered index.
    /// One tool at a time on the canvas — the SINGLE owner of the disarm
    /// list. Ten arming sites used to hand-copy these assignments, and the
    /// comments at two of them record real bugs born from copies drifting;
    /// every arming site now calls this before setting its own flag.
    /// Transient gesture anchors die with the tools: a stale place_start or
    /// crop_drag used to hijack the next drag. clone_src (the sampled source
    /// pin) deliberately survives — samples stay for resuming, like paint.
    pub(crate) fn disarm_tools(&mut self) {
        self.workspace.disarm_tools(self.editor);
    }
    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_detail(&mut self, ui: &mut egui::Ui, detail_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        // Look ends here — detail, then geometry (lens BEFORE crop: the lens
        // profile / manual distortion redefine the frame the crop sits in).
        // Group boundary: the group head's own air and gold caption (2026-10-01:
        // the full-width hairline that used to fence each group is gone).
        group_caption(ui, tr(lang, K_DEVELOP_DETAIL_LENS)); // #14b, group 2 of 4 here
        section_fold(ui, "sec_detail", section_title(tr(lang, K_DEVELOP_DETAIL), detail_active), false, false, |ui| {
                {
                    // All eleven controls render since v1.5.0
                    // (`render/detail.rs`), so none carries the "not rendered
                    // here" line any more. The six COMPANIONS (the amount since
                    // v1.6.0 — Lightroom's 40 on a RAW, none on a baked raster —
                    // radius, the three Details, Smoothness) show the value the
                    // engine renders — Lightroom's default while the recipe
                    // holds none — and reset to it (`companion_slider_edit`,
                    // `sharpening_slider`).
                    let raw_source = self.editor.source_is_raw();
                    changed |= sharpening_slider(ui, lang, tr(lang, K_DEVELOP_SHARPENING), &mut self.editor.doc_mut().recipe, raw_source);
                    // The rows below the amount are one capability table each;
                    // their bands, feels and companion defaults live in
                    // `logic::capability` (the radius is a Fine companion: the
                    // sidecar writes one decimal, so the track shows one).
                    changed |= self.capability_rows(ui, crate::controls::SHARPEN);
                    ui.add_space(SPACE_SM);
                    changed |= self.capability_rows(ui, crate::controls::NOISE_LUMA);
                    ui.add_space(SPACE_SM);
                    changed |= self.capability_rows(ui, crate::controls::NOISE_COLOR);
                }
                let denoise_raw = self.editor.denoise_uses_raw();
                // AI denoise as an ACTIVE op: run now, see it on canvas —
                // export-time denoise (the Export section toggle) stays for
                // batch/full-res workflows, but nobody should have to export
                // to find out what the denoiser does.
                ui.add_space(SPACE_SM);
                // The verb's OWN dial, directly above it (user decision
                // 2026-09-12: a control belongs to the fold whose function it
                // serves; the Export fold's 「on export」 has a dial of its
                // own, and neither reaches the other). Same helper as the
                // reverse-fit's: 0..100 track, double-click reset, hover nudge.
                slider_pct_hinted(
                    ui,
                    lang,
                    tr(lang, K_DEVELOP_AI_DENOISE_STRENGTH),
                    if denoise_raw { &mut self.workspace.exp_mut().denoise_strength } else { &mut self.workspace.exp_mut().baked_denoise_strength },
                    1.0,
                    if denoise_raw { autoshade::denoise::DEFAULT_STRENGTH_RAW } else { autoshade::denoise::DEFAULT_STRENGTH },
                    tr(lang,
                        K_DEVELOP_DETAIL_AI_DENOISE_STRENGTH_INDEPENDENT_OF_THE_EX,
                    ),
                );
                ui.horizontal(|ui| {
                    // CAPABILITY, not just state — the same rule the two
                    // segmentation buttons follow one panel down (R24 batch 2).
                    // Without `python/denoise.py` on disk this click can only
                    // end in `denoise.rs`'s English "denoise sidecar not found
                    // at …" landing in a status line the rest of the window
                    // renders in the user's language, so the button says so up
                    // front instead of spending the click to find out.
                    let has_helper = denoise_helper_available();
                    let ready = self.editor.photo().src_path.is_some() && !self.workspace.busy() && has_helper;
                    let missing = tr(lang,
                        K_DEVELOP_THIS_FEATURE_NEEDS_AUTOSHADE_S_PYTHON_HELPERS_WH,
                    );
                    if action_row(ui, ready, tr(lang, K_DEVELOP_AI_DENOISE_NOW))
                        // 🤖 + the cross-reference line (#4): this verb stays
                        // beside Noise Reduction on purpose, so its tooltip is
                        // where it says the rest of the AI moved to. On the arm
                        // that CANNOT run, the tooltip is about the missing
                        // sidecar and nothing else — same discipline as the
                        // segmentation pair.
                        .on_hover_text(if has_helper { ai_xref(lang, tr(lang,
                            K_DEVELOP_DENOISE_THIS_CARD_S_PIXELS_AT_FULL_RESOLUTION_A,
                        )) } else { missing.to_string() })
                        .clicked()
                    {
                        self.act_all(vec![logic::Action::Develop(
                            logic::develop::DevelopAction::AiDenoise,
                        )]);
                    }
                    // No 「Full-res」 checkbox any more (2026-09-15): the
                    // result is a CARD, and a card baked from a ≤2048 px
                    // working copy capped every later export of it at
                    // 2048 px — the denoise always runs on the full frame.
                });
            });
        changed
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    ///
    /// v1.5.0: the section is Lightroom's WHOLE Color Mixer. The Black & White
    /// switch swaps the colour bands for the grey mix, because the render runs
    /// one or the other (`render::apply_gray_mix` in the colour mixer's place,
    /// as in Lightroom) and showing both would be eight sliders that move no
    /// pixel; the point colours follow the mixer ([`Self::point_color_rows`]).
    pub(crate) fn dev_hsl(&mut self, ui: &mut egui::Ui, hsl_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_MD);
        section_fold(ui, "sec_hsl", section_title(tr(lang, K_DEVELOP_COLOR_MIXER_HSL), hsl_active), false, false, |ui| {
                changed |= ui
                    .checkbox(&mut self.editor.doc_mut().recipe.convert_to_grayscale, tr(lang, K_DEVELOP_BLACK_WHITE))
                    .changed();
                // What the CANVAS does, not what the checkbox says (v1.5.0 F7):
                // a monochrome creative profile develops the photograph to grey
                // with the photographer's own switch still off, and a panel
                // offering the colour mixer over a grey canvas is a row of
                // sliders that move numbers and no pixels. `render::apply_develop`
                // asks the same question the same way.
                let mono = self.editor.doc().recipe.renders_grayscale();
                // LR's mixer layout: pick ONE property (Hue / Saturation /
                // Luminance) and see all 8 bands at once. The old band picker
                // showed one band at a time — every other band's state was
                // invisible, the opposite of what a mixer is for. The B&W
                // mixer has ONE property (how light each colour turns), so it
                // has no tabs — its caption stands where they do.
                ui.horizontal(|ui| {
                    if mono {
                        ui.label(Text::new(tr(lang, K_DEVELOP_B_W_MIX)).weak().small());
                    } else {
                        for (i, name) in ["Hue", "Saturation", "Luminance"].iter().enumerate() {
                            if ui.selectable_label(self.workspace.ai().hsl_tab == i, tr(lang, name)).clicked() {
                                self.workspace.ai_mut().hsl_tab = i;
                            }
                        }
                    }
                    // R38: a glyph square — the same reset in every mixer row;
                    // as a text button it ran the HSL tab row to 318 px at the
                    // 320 px default and widened the auto-fitting panel.
                    if glyph(ui, true, "↺").on_hover_text(tr(lang, K_DEVELOP_RESET_ALL)).clicked() {
                        if mono {
                            for b in 0..HSL_BANDS.len() {
                                if let Some(v) = self.editor.doc_mut().recipe.gray_mixer_mut(b) {
                                    *v = 0.0;
                                }
                            }
                        } else {
                            self.editor.doc_mut().recipe.hsl = Hsl::default();
                        }
                        changed = true;
                    }
                });
                let tab = self.workspace.ai().hsl_tab;
                for (b, name) in HSL_BANDS.iter().enumerate() {
                    // ONE row shape for both mixers: same band, same swatch,
                    // same ±100 track — only the number behind it changes.
                    let v = match (mono, tab) {
                        (true, _) => match self.editor.doc_mut().recipe.gray_mixer_mut(b) {
                            Some(v) => v,
                            None => continue,
                        },
                        (false, 0) => &mut self.editor.doc_mut().recipe.hsl.hue[b],
                        (false, 1) => &mut self.editor.doc_mut().recipe.hsl.saturation[b],
                        (false, _) => &mut self.editor.doc_mut().recipe.hsl.luminance[b],
                    };
                    ui.horizontal(|ui| {
                        // Band swatch so rows scan like LR's mixer.
                        let (rect, _) = ui
                            .allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, HSL_SWATCH[b]);
                        changed |= slider(ui, lang, tr(lang, name), v, -100.0, 100.0, 0.0);
                    });
                }
                changed |= self.point_color_rows(ui);
            });
        changed
    }

    /// Lightroom's Point Color (v1.5.0), under the mixer as in Lightroom: the
    /// 💧 that arms the canvas eyedropper (`canvas::handle_point_color_pick`
    /// makes the swatch), then one block per swatch — the colour it was
    /// sampled from, its three shifts and its Range.
    ///
    /// Disabled under Black & White, where `apply_gray_mix` runs in the
    /// mixer's place and a swatch has no colour left to move.
    ///
    /// The Range slider goes through [`autoshade::recipe::PointColor::set_range`],
    /// which REBUILDS the swatch's three windows: the engine matches pixels
    /// against the stored windows and never reads `range_amount`, so a Range
    /// that only wrote its own number would be a slider that moves no pixel.
    pub(crate) fn point_color_rows(&mut self, ui: &mut egui::Ui) -> bool {
        use autoshade::recipe::MAX_POINT_COLORS;
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_SM);
        // Point Color has no colour to act on in black and white, whichever of
        // the two switches produced it (v1.5.0 F7 — see the mixer above).
        ui.add_enabled_ui(!self.editor.doc().recipe.renders_grayscale(), |ui| {
            ui.horizontal_wrapped(|ui| {
                // One cell of the grid beside the caption (2026-09-27).
                let cell = columns(ui, 2);
                ui.label(Text::new(tr(lang, K_DEVELOP_POINT_COLOR)).weak().small());
                let label = if self.workspace.tools().point_color_picking {
                    tr(lang, K_DEVELOP_CLICK_IN_IMAGE)
                } else {
                    tr(lang, K_DEVELOP_PICK_A_COLOR)
                };
                let room = self.editor.doc().recipe.point_colors.len() < MAX_POINT_COLORS;
                if action_in(ui, cell, room, label)
                    .on_hover_text(tr(lang,
                        K_DEVELOP_CLICK_A_COLOUR_IN_THE_IMAGE_TO_ADD_A_SWATCH_ITS,
                    ))
                    // The limit says itself where the button that hit it is —
                    // the canvas handler repeats it for the click that arrives
                    // after a paste filled the list behind an armed tool.
                    .on_disabled_hover_text(trf(
                        lang,
                        K_LOGIC_POINT_COLOR_AT_MOST_SWATCHES,
                        &[("n", &MAX_POINT_COLORS.to_string())],
                    ))
                    .clicked()
                {
                    let on = !self.workspace.tools().point_color_picking;
                    self.disarm_tools();
                    self.workspace.tools_mut().point_color_picking = on;
                    if on {
                        self.workspace.set_status(tr(lang, K_DEVELOP_POINT_COLOR_EYEDROPPER_CLICK_THE_COLOUR_TO_ADJUS).into());
                    }
                }
            });
            // Deferred like every other list row's ✕ (R24-5): removing inside
            // the loop would invalidate the iterator it is drawn from.
            let mut remove = None;
            for (i, p) in self.editor.doc_mut().recipe.point_colors.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    // The colour the swatch was sampled from, as a chip: the
                    // sliders below say what happens to it, and nothing else
                    // on the row says WHICH colour it is.
                    let rgb = autoshade::render::point_color_rgb(p)
                        .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(
                        rect,
                        2.0,
                        egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                    );
                    ui.label(trf(lang, K_DEVELOP_SWATCH, &[("n", &(i + 1).to_string())]));
                    // 🗑, the vocabulary's DELETE-a-list-row glyph (masks,
                    // versions) — ✕ is "clear / close" and says the wrong thing
                    // about a swatch that is gone for good.
                    if glyph(ui, true, "🗑").on_hover_text(tr(lang, K_DEVELOP_REMOVE_SWATCH)).clicked() {
                        remove = Some(i);
                    }
                });
                // Lightroom's ±100 tracks over the stored -1..=1 shifts.
                for (name, shift) in [
                    ("Hue", &mut p.hue_shift),
                    ("Saturation", &mut p.sat_scale),
                    ("Luminance", &mut p.lum_scale),
                ] {
                    let mut shown = *shift * 100.0;
                    if slider(ui, lang, tr(lang, name), &mut shown, -100.0, 100.0, 0.0) {
                        *shift = shown / 100.0;
                        changed = true;
                    }
                }
                let mut range = p.range_amount;
                if slider_pct(ui, lang, tr(lang, K_DEVELOP_RANGE), &mut range, 1.0, 0.5) {
                    p.set_range(range);
                    changed = true;
                }
            }
            if let Some(i) = remove {
                self.editor.doc_mut().recipe.point_colors.remove(i);
                changed = true;
            }
        });
        changed
    }
    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_grading(&mut self, ui: &mut egui::Ui, grade_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        ui.add_space(SPACE_MD);
        section_fold(ui, "sec_grade", section_title(tr(lang, K_DEVELOP_COLOR_GRADING), grade_active), false, false, |ui| {
                // Lightroom's grading panel (2026-10-13): the 3-way view shows
                // the shadow, midtone and highlight wheels together, each with
                // its luminance under it; Global is one wheel for the whole
                // picture (`panels/grade_wheel.rs`).
                ui.horizontal(|ui| {
                    let gap = ui.spacing().item_spacing.x;
                    let w = ((ui.available_width().min(FIELD_W_MAX) - row_h(ui) - 2.0 * gap) / 2.0).floor();
                    for (i, name) in GRADE_VIEWS.iter().enumerate() {
                        if toggle_in(ui, egui::vec2(w, row_h(ui)), self.workspace.ai().grade_region == i, tr(lang, name)).clicked() {
                            self.workspace.ai_mut().grade_region = i;
                        }
                    }
                    // R38: a glyph square — the same reset in every mixer row.
                    if glyph(ui, true, "↺").on_hover_text(tr(lang, K_DEVELOP_RESET_ALL)).clicked() {
                        self.editor.doc_mut().recipe.color_grade = ColorGrade::default();
                        changed = true;
                    }
                });
                ui.add_space(SPACE_SM);
                let cg = &mut self.editor.doc_mut().recipe.color_grade;
                // One wheel with its luminance slider in a column `width` wide.
                let wheel = |ui: &mut egui::Ui, title: &str, width: f32, h: &mut f32, s: &mut f32, l: &mut f32| {
                    let mut c = false;
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        c |= grade_wheel(ui, lang, title, (width - 8.0).min(170.0), h, s);
                        c |= slider(ui, lang, tr(lang, K_DEVELOP_LUMINANCE), l, -100.0, 100.0, 0.0);
                    });
                    c
                };
                let avail = ui.available_width();
                if self.workspace.ai().grade_region == 0 {
                    // Midtones on top, centred; shadows and highlights under it.
                    let half = ((avail - ui.spacing().item_spacing.x) / 2.0).floor();
                    ui.horizontal(|ui| {
                        ui.add_space(((avail - half) / 2.0).floor());
                        changed |= wheel(ui, tr(lang, K_DEVELOP_MIDTONES), half, &mut cg.midtone_hue, &mut cg.midtone_sat, &mut cg.midtone_lum);
                    });
                    ui.add_space(SPACE_SM);
                    ui.horizontal(|ui| {
                        changed |= wheel(ui, tr(lang, K_DEVELOP_SHADOWS), half, &mut cg.shadow_hue, &mut cg.shadow_sat, &mut cg.shadow_lum);
                        changed |= wheel(ui, tr(lang, K_DEVELOP_HIGHLIGHTS), half, &mut cg.highlight_hue, &mut cg.highlight_sat, &mut cg.highlight_lum);
                    });
                } else {
                    let width = (avail * 0.7).floor();
                    ui.horizontal(|ui| {
                        ui.add_space(((avail - width) / 2.0).floor());
                        changed |= wheel(ui, tr(lang, K_DEVELOP_GLOBAL), width, &mut cg.global_hue, &mut cg.global_sat, &mut cg.global_lum);
                    });
                }
                // Blending/Balance shape the WHOLE grade, not one wheel — a
                // scope caption keeps them from reading as a wheel's own. The
                // two sliders are capability rows: the band and the reset
                // (blending 50, not 0) come from the core catalogue's nested
                // colour-grade table, not from this draw site.
                sub_caption(ui, tr(lang, K_DEVELOP_ALL_REGIONS));
                changed |= self.capability_rows(ui, crate::controls::GRADE_GLOBALS);
            });
        changed
    }

    /// ▦ 堆栈 — several frames of one scene merged into a new negative.
    ///
    /// The ACTIVE card's frame is the reference and the picked frames join it;
    /// the result is a new ▦ card. Every input the verb reads is drawn in this
    /// fold (the user decision of 2026-09-12): which merge to run, and whether
    /// the frames are registered already.
    ///
    /// It returns nothing because it changes no recipe field — the merge
    /// writes a master and pushes a card, which is not an edit of the develop
    /// the panel is holding.
    pub(crate) fn dev_stack(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        let lang = self.lang;
        // The section's ● says this photo HAS a stacked card, which is the
        // only state this fold owns that outlives the click.
        let has = self.editor.doc().variants.iter().any(|v| v.kind == VariantKind::Stacked);
        ui.add_space(SPACE_MD);
        section_fold(ui, "sec_stack", section_title(tr(lang, K_DEVELOP_STACK), has), false, false, |ui| {
                explainer(ui, lang, tr(lang, K_DEVELOP_MERGE_SEVERAL_FRAMES_OF_ONE_SCENE_INTO_A_NEW_NEG));
                ui.add_space(SPACE_SM);
                egui::ComboBox::from_id_salt("stack_kind")
                    .selected_text(tr(lang, self.workspace.tools().stack_kind.label()))
                    .show_ui(ui, |ui| {
                        for k in autoshade::stack::merge::StackKind::ALL {
                            ui.selectable_value(&mut self.workspace.tools_mut().stack_kind, k, tr(lang, k.label()));
                        }
                    })
                    .response
                    .on_hover_text(tr(
                        lang,
                        K_DEVELOP_HDR_MERGE_AN_EXPOSURE_BRACKET_INTO_ONE_FRAME_HOL,
                    ));
                ui.checkbox(&mut self.workspace.tools_mut().stack_tripod, tr(lang, K_DEVELOP_SHOT_ON_A_TRIPOD_SKIP_ALIGNMENT))
                    .on_hover_text(tr(
                        lang,
                        K_DEVELOP_ALIGNMENT_IS_THE_EXPENSIVE_HALF_OF_A_STACK_AND_O,
                    ));
                ui.add_space(SPACE_SM);
                ui.horizontal(|ui| {
                    let ready = self.editor.photo().src_path.is_some() && !self.workspace.busy();
                    if action_row(ui, ready, tr(lang, K_DEVELOP_STACK_WITH_OTHER_FRAMES))
                        .on_hover_text(tr(
                            lang,
                            K_DEVELOP_PICK_THE_OTHER_FRAMES_OF_THIS_SCENE_THEY_MUST_BE,
                        ))
                        .clicked()
                        && let Some(extra) = photo_files_dialog()
                    {
                        self.act_all(vec![logic::Action::Develop(
                            logic::develop::DevelopAction::Stack(extra),
                        )]);
                    }
                });
            });
            false
    }

    /// The Lens Blur section of the Develop panel.
    pub(crate) fn dev_lens_blur(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        // A verb section: its body returns the actions the worker start
        // needs, and the row's draw applies them (the facade does this for
        // every surface).
        let actions = self.lens_blur_section(ui);
        let touched = !actions.is_empty();
        if touched {
            self.act_all(actions);
        }
        touched
    }

    pub(crate) fn lens_blur_section(&mut self, ui: &mut egui::Ui) -> Vec<logic::Action> {
        let lang = self.lang;
        let active = logic::lens_blur::active(&self.editor.doc().recipe);
        let mut actions = Vec::new();
        ui.add_space(SPACE_MD);
        section_fold(ui, "sec_lens_blur", section_title(tr(lang, K_LENS_LENS_BLUR), active), false, false, |ui| actions = self.lens_blur_body(ui),
        );
        actions
    }

    pub(crate) fn lens_blur_body(&mut self, ui: &mut egui::Ui) -> Vec<logic::Action> {
        let lang = self.lang;
        let mut actions = Vec::new();
        let has_helper = depth_helper_available();
        let can_run = !self.workspace.busy() && self.editor.photo().base_preview.is_some() && self.editor.photo().src_path.is_some() && has_helper;
        let has_blur = self.editor.doc().recipe.lens_blur.is_some();
        let verb = if has_blur { tr(lang, K_LENS_RE_ESTIMATE_DEPTH) } else { tr(lang, K_LENS_ESTIMATE_DEPTH) };
        let tip = if has_helper {
            ai_xref(lang, tr(lang, K_LENS_MOGE_2_ESTIMATES_HOW_FAR_AWAY_EVERY_PART_OF_THE))
        } else {
            tr(lang, K_LENS_THIS_FEATURE_NEEDS_AUTOSHADE_S_PYTHON_HELPER_WHI).to_string()
        };
        if action_row(ui, can_run, verb).on_hover_text(tip).clicked() {
            actions.push(logic::Action::Lens(logic::lens_blur::LensAction::EstimateDepth));
        }
        // The card's current values, copied out so the draw below never holds a
        // borrow into the recipe the facade is about to change.
        let Some((enabled, amount, range)) = self.editor.doc().recipe.lens_blur.as_ref().map(|l| (l.enabled, l.amount, l.range)) else {
            let hint = tr(lang, K_LENS_ESTIMATE_THE_DEPTH_FIRST_THEN_CLICK_THE_PHOTO_TO);
            ui.label(Text::new(hint).weak().small());
            return actions;
        };
        let picking = self.workspace.tools().lens_focus_picking;
        ui.horizontal(|ui| {
            let mute = tr(lang, K_LENS_SHOW_MUTE_THE_LENS_BLUR_WITHOUT_LOSING_ITS_SETTI);
            if glyph_toggle(ui, enabled, "👁").on_hover_text(mute).clicked() {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::ToggleLensBlur(!enabled)));
            }
            let cell = egui::vec2(ui.available_width() - row_h(ui) - ui.spacing().item_spacing.x, row_h(ui));
            let how = tr(lang, K_LENS_CLICK_THE_PHOTO_WHATEVER_IS_AT_THAT_DISTANCE_STA);
            if toggle_in(ui, cell, picking, tr(lang, K_LENS_PICK_FOCUS)).on_hover_text(how).clicked() {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::SetFocusPicking(!picking)));
            }
            if glyph(ui, true, "🗑").on_hover_text(tr(lang, K_LENS_REMOVE_THE_LENS_BLUR)).clicked() {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::RemoveLensBlur));
            }
        });
        // The two dials of `render::LensBlur`; the copy is what the view may
        // touch, the recipe change goes through the action.
        ui.add_enabled_ui(enabled, |ui| {
            let mut next = amount;
            if slider(ui, lang, tr(lang, K_LENS_BLUR_AMOUNT), &mut next, 0.0, 100.0, 50.0) {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::SetBlurAmount(next)));
            }
            let mut next = range;
            if slider(ui, lang, tr(lang, K_LENS_FOCUS_RANGE), &mut next, 0.0, 100.0, 10.0) {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::SetFocusRange(next)));
            }
        });
        let carried = tr(lang, K_LENS_RENDERED_HERE_AND_IN_EVERY_EXPORT_FROM_THIS_APP);
        ui.label(Text::new(carried).weak().small());
        actions
    }
    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_presence(&mut self, ui: &mut egui::Ui, presence_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        // LR-Basic order (UX batch): Presence sits directly under Tone & WB —
        // the two halves of Lightroom's Basic panel — then Curves, then the
        // two colour sections; Detail moved BELOW them so colour work isn't
        // interrupted by sharpening. add_space fences each header so the ten
        // sections read as groups, not one undivided list.
        ui.add_space(SPACE_MD);
        section_fold(ui, "sec_presence", section_title(tr(lang, K_DEVELOP_PRESENCE), presence_active), true, false, |ui| {
                // Lightroom's Basic-panel order inside Presence is Texture →
                // Clarity → Dehaze, and the engine runs them in that order too
                // (R25 B2 put the global texture stage between clarity and
                // saturation). The rows live in `logic::capability::PRESENCE`;
                // rows and order are one table.
                changed |= self.capability_rows(ui, crate::controls::PRESENCE);
            });
        changed
    }

    /// 校准 — Lightroom's Calibration panel (v1.5.0): the shadows tint and each
    /// primary's hue and saturation.
    ///
    /// Rendered as the develop's FIRST stage (`render::Calibration`), which is
    /// why it is a section of sliders and not a read-out: every other section
    /// works on the colours these seven leave behind. Lightroom writes all
    /// seven keys on nearly every sidecar, so a photo arriving from Lightroom
    /// usually lands here with values already in it.
    pub(crate) fn dev_calibration(&mut self, ui: &mut egui::Ui, calibration_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_MD);
        section_fold(ui, "sec_calibration", section_title(tr(lang, K_DEVELOP_CALIBRATION), calibration_active), false, false, |ui| {
                // Lightroom's own order and grouping: the shadows tint first,
                // then a hue + saturation pair per primary. The group captions
                // stay here; the rows are capability tables.
                ui.label(Text::new(tr(lang, K_DEVELOP_SHADOWS)).weak().small());
                changed |= self.capability_rows(ui, crate::controls::CAL_TINT);
                for (primary, table) in [
                    (tr(lang, K_DEVELOP_RED_PRIMARY), crate::controls::CAL_RED),
                    (tr(lang, K_DEVELOP_GREEN_PRIMARY), crate::controls::CAL_GREEN),
                    (tr(lang, K_DEVELOP_BLUE_PRIMARY), crate::controls::CAL_BLUE),
                ] {
                    ui.add_space(SPACE_XS);
                    ui.label(Text::new(primary).weak().small());
                    changed |= self.capability_rows(ui, table);
                }
            });
        changed
    }

    /// 效果 — Lightroom's post-crop vignette and film grain.
    ///
    /// All nine RENDER since v1.5.0 (`render/finish.rs`, the stage this engine
    /// grew after its crop), so none of them carries the "moves a number and
    /// no pixel" disclosure any more. The five COMPANIONS (Midpoint, Feather,
    /// Style, Grain size and roughness) show the value the engine actually
    /// renders — Lightroom's own default while the recipe holds none — and a
    /// reset returns them to absent (`companion_slider_edit` + the command).
    pub(crate) fn dev_effects(&mut self, ui: &mut egui::Ui, effects_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        ui.add_space(SPACE_MD);
        section_fold(ui, "sec_effects", section_title(tr(lang, K_DEVELOP_EFFECTS), effects_active), false, false, |ui| {
                ui.label(Text::new(tr(lang, K_DEVELOP_POST_CROP_VIGNETTING)).weak().small());
                // Two tables because the panel draws two captioned groups; the
                // rows, their bands (the catalogue), their feels and their
                // companion encoding live in `logic::capability`. The style row
                // is an operator INDEX (1/2/3); the track starts at 1 because
                // 0 means "the sidecar named none" and only a reset reaches it.
                changed |= self.capability_rows(ui, crate::controls::VIGNETTE);
                ui.add_space(SPACE_SM);
                ui.label(Text::new(tr(lang, K_DEVELOP_GRAIN)).weak().small());
                changed |= self.capability_rows(ui, crate::controls::GRAIN);
            });
        changed
    }

    /// v1.5.0 F8 — Lightroom's HDR edit mode and the SDR rendition it is
    /// published through.
    ///
    /// The switch is drawn FIRST and the seven controls under a heading that
    /// says they need it, because that is the one thing a photographer can get
    /// wrong here: the seven are live widgets whatever the switch says (they
    /// are stored either way, and Lightroom's own sidecars keep them across a
    /// mode change), but nothing they do reaches a pixel while the mode is
    /// off. Greying them out instead would hide values the file really holds.
    pub(crate) fn dev_hdr(&mut self, ui: &mut egui::Ui, hdr_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_MD);
        section_fold(ui, "sec_hdr", section_title(tr(lang, K_DEVELOP_HDR_SDR), hdr_active), false, false, |ui| {
                if ui
                    .checkbox(&mut self.editor.doc_mut().recipe.hdr_edit, tr(lang, K_DEVELOP_HDR_EDIT_MODE))
                    .on_hover_text(tr(
                        lang,
                        K_DEVELOP_THIS_PHOTOGRAPH_WAS_EDITED_ABOVE_DIFFUSE_WHITE_E,
                    ))
                    .changed()
                {
                    changed = true;
                }
                changed |= self.capability_rows(ui, crate::controls::HDR_HEADROOM);
                ui.add_space(SPACE_SM);
                ui.label(Text::new(tr(lang, K_DEVELOP_SDR_RENDITION)).weak().small());
                ui.label(
                    Text::new(tr(lang, K_DEVELOP_RENDERS_ONLY_IN_HDR_MODE))
                        .weak()
                        .small(),
                );
                changed |= self.capability_rows(ui, crate::controls::HDR_SDR);
            });
        changed
    }

    /// 变换 — Lightroom's Transform panel, RENDERED since v1.5.0 F6, plus what
    /// is still carried verbatim beside it.
    ///
    /// R25 B4 drew this section read-only, and said why: pass-through meant no
    /// band, no clamp, no neutral and no idea what a value did to a pixel, and a
    /// slider needs all four. F6 supplies all four — the eight `crs:Perspective*`
    /// keys are owned controls with measured neutrals, and
    /// `render::perspective` is the stage that moves the pixels — so the sliders
    /// are now honest. What remains carried is the ASSUMPTIONS Lightroom's own
    /// Upright solver worked from (its version stamp, and the centre and focal
    /// length it took the photograph to have) and the camera profile's name,
    /// and those keep the old treatment: Adobe's own property spelling, and a
    /// sentence saying we do not interpret them. Its cached ANSWER — the
    /// matrices, their count, its preview flag and its staleness digest — is
    /// not carried at all; see [`autoshade::xmp::PASSTHROUGH_CRS`] for why.
    ///
    /// (The Calibration panel used to share this heading. It renders since
    /// v1.5.0 — `dev_calibration` above — and was never in this map: R25
    /// listed seven `CameraCalibration*` keys Lightroom does not write.)
    pub(crate) fn dev_transform(&mut self, ui: &mut egui::Ui, transform_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_MD);
        section_fold(ui, "sec_transform", section_title(tr(lang, K_DEVELOP_TRANSFORM), transform_active), false, false, |ui| {
                // Upright is a MODE, so a dropdown and not a slider — and the
                // one control in this panel whose answer may come from either
                // side: Lightroom publishes the matrix its own solver found
                // (`crs:UprightTransform_N`), and when the photo carries none
                // for the chosen mode this engine solves it
                // (`render::perspective::solve_upright`).
                const MODES: [&str; 6] =
                    ["Off", "Auto", "Level", "Vertical", "Full", "Guided"];
                let mut mode = self.editor.doc().recipe.perspective_upright.round().clamp(0.0, 5.0) as usize;
                ui.horizontal(|ui| {
                    ui.label(Text::new(tr(lang, K_DEVELOP_UPRIGHT)).small());
                    egui::ComboBox::from_id_salt("upright_mode")
                        .selected_text(tr(lang, MODES[mode]))
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for (i, name) in MODES.iter().enumerate() {
                                ui.selectable_value(&mut mode, i, tr(lang, name));
                            }
                        });
                });
                if mode as f32 != self.editor.doc().recipe.perspective_upright {
                    self.editor.doc_mut().recipe.perspective_upright = mode as f32;
                    changed = true;
                }
                // Which side will answer, said out loud — the difference
                // between "Adobe's numbers to the last digit" and "our own
                // solver, which may differ slightly" is exactly what a
                // photographer comparing the two apps needs to know.
                if mode > 0 {
                    let adobes = self.editor.doc().recipe
                        .upright_transform
                        .get(mode)
                        .is_some_and(|m| *m != [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
                    // Named, in ONE array, for the i18n audit's dynamic-key
                    // registry: a fourth answer added inline would reach the
                    // screen with no zh pair and nothing would say so.
                    const UPRIGHT_NOTES: [&str; 3] = [
                        "Rendering the matrix Lightroom solved for this photo",
                        "Guided needs the guides drawn in Lightroom; only the matrix it wrote can be rendered",
                        "Lightroom solved no matrix for this photo, so AutoShade solves it from the picture's own lines",
                    ];
                    // `upright_note`, not `note`: the i18n audit registers a
                    // dynamic call site by the SPELLING of its argument, and a
                    // bare `note` would have been a pattern wide enough to
                    // accept some future unaudited key somewhere else.
                    let upright_note = if adobes {
                        UPRIGHT_NOTES[0]
                    } else if mode == 5 {
                        UPRIGHT_NOTES[1]
                    } else {
                        UPRIGHT_NOTES[2]
                    };
                    ui.label(Text::new(tr(lang, upright_note)).weak().small());
                }
                ui.add_space(SPACE_SM);
                // One capability table: bands and resets (Rotate ±10, scale
                // 50..150 resetting to 100) come from `logic::capability`.
                changed |= self.capability_rows(ui, crate::controls::TRANSFORM);
                if ui
                    .checkbox(&mut self.editor.doc_mut().recipe.crop_constrain_to_warp, tr(lang, K_DEVELOP_CONSTRAIN_CROP))
                    .on_hover_text(tr(lang, K_DEVELOP_SHRINK_THE_CROP_UNTIL_IT_LIES_INSIDE_THE_WARPED))
                    .changed()
                {
                    changed = true;
                }
                ui.add_space(SPACE_SM);
                explainer(ui, lang, tr(lang, K_DEVELOP_ONE_PROJECTIVE_MAP_AFTER_THE_LENS_CORRECTION_AND));
                // v1.5.0 F7 — WHICH rendering this photograph starts from.
                // Read-only here on purpose: the profile is a file on the
                // photographer's machine and the Look is Adobe's own creative
                // block, so the panel's job is to say what is being used, not
                // to offer a picker for something the sidecar decides.
                let look = self.editor.doc().recipe.look.clone();
                if !self.editor.doc().recipe.camera_profile.is_empty() || look.is_some() {
                    ui.add_space(SPACE_MD);
                    let pair = |ui: &mut egui::Ui, label: String, value: &str| {
                        ui.horizontal(|ui| {
                            ui.label(Text::new(label).small());
                            ui.label(Text::new(value).small().strong());
                        });
                    };
                    if !self.editor.doc().recipe.camera_profile.is_empty() {
                        pair(
                            ui,
                            tr(lang, K_DEVELOP_CAMERA_PROFILE).to_string(),
                            &self.editor.doc().recipe.camera_profile,
                        );
                    }
                    if let Some(look) = look.as_ref().filter(|l| !l.name.is_empty()) {
                        pair(ui, tr(lang, K_DEVELOP_CREATIVE_PROFILE).to_string(), &look.name);
                        // Named, not hidden: a Look is colour data (v1.17.0) —
                        // its creative colour table renders when Camera Raw's
                        // install carries it (`dcp::find_look_table`,
                        // answered once per process) and the Look is at full
                        // amount, the render's own two conditions; its baked
                        // tone curve and sliders never do. A panel that showed
                        // the name without saying which half arrived would be
                        // claiming the whole profile.
                        let line = if look.table.is_empty() {
                            tr(lang, K_DEVELOP_ITS_BAKED_TONE_CURVE_AND_SLIDERS_DO_NOT_RENDER)
                        } else if (look.amount - 1.0).abs() <= 1e-3
                            && autoshade::dcp::find_look_table(&look.table).is_ok()
                        {
                            tr(lang, K_DEVELOP_ITS_CREATIVE_COLOUR_TABLE_RENDERS_ITS_BAKED_TONE)
                        } else {
                            tr(lang, K_DEVELOP_NEITHER_ITS_CREATIVE_COLOUR_TABLE_NOR_ITS_BAKED)
                        };
                        ui.label(Text::new(line).weak().small());
                    }
                }
                if self.editor.doc().recipe.passthrough.is_empty() {
                    return;
                }
                ui.add_space(SPACE_MD);
                // In PASSTHROUGH_CRS order (Adobe's own grouping), which is
                // also the order the writer emits — one order everywhere, so
                // the panel and the sidecar read the same way.
                let rows = |ui: &mut egui::Ui, caption: Option<&str>, keys: &[&str]| {
                    let present: Vec<(&str, &str)> = keys
                        .iter()
                        .filter_map(|k| {
                            self.editor.doc().recipe.passthrough.get(*k).map(|v| (*k, v.as_str()))
                        })
                        .collect();
                    if present.is_empty() {
                        return;
                    }
                    if let Some(caption) = caption {
                        ui.label(Text::new(caption).weak().small());
                    }
                    for (key, value) in present {
                        ui.horizontal(|ui| {
                            ui.label(Text::new(format!("crs:{key}")).small());
                            ui.label(Text::new(value).small().strong());
                        });
                    }
                };
                // Every carried key, in the block's own order. The partition
                // this used to make had two groups because the profile NAME was
                // carried alongside the solver's bookkeeping; F7 made the name
                // an owned control shown above, so what is left is one block
                // with one caption.
                rows(ui, Some(tr(lang, K_DEVELOP_UPRIGHT_SOLVER_BOOKKEEPING)), &autoshade::xmp::PASSTHROUGH_CRS);
                ui.add_space(SPACE_SM);
                ui.label(
                    Text::new(tr(
                        lang,
                        K_DEVELOP_CARRIED_THROUGH_TO_THE_SIDECAR_UNCHANGED_AUTOSHA,
                    ))
                    .weak()
                    .small(),
                );
            });
        changed
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_lens(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        // --- 镜头校正: in-camera profile + manual corrections -----------------
        ui.add_space(SPACE_MD);
        // Two as-opened states are neutral here: the camera's available
        // components on, or all off with the sidecar's DisabledInSidecar stamp.
        // Neither lights the dot; hand-made all-off and other moves away do.
        // This header rule leaves is_noop's saved-develop precedence and
        // unsaved badge alone. The `lens` / `lens_effects` families still cover
        // manual sliders, including the two profile strengths; `DOT_EXEMPT`
        // keeps midpoints and hue windows quiet with their amounts at rest.
        let lens_active = CONTROL_FAMILIES
            .iter()
            .filter(|f| f.name == "lens" || f.name == "lens_effects")
            .any(|f| family_is_active(f, &self.editor.doc().recipe))
            || !self.editor.doc().recipe.lens_profile.is_as_opened();
        section_fold(ui, "sec_lens", section_title(tr(lang, K_DEVELOP_LENS), lens_active), false, false, |ui| {
                // In-camera profile corrections (lensmeta): the manufacturer's
                // exact per-shot data read from the RAW itself — no lens
                // database. Toggling a component ON whose data a legacy
                // restore dropped re-fills it from this photo's metadata.
                let photo = self.workspace.open().photo_lens.clone();
                let lp = &mut self.editor.doc_mut().recipe.lens_profile;
                let has_v = !lp.vignette.is_empty() || !photo.vignette.is_empty();
                let has_d = !lp.distortion.is_empty() || !photo.distortion.is_empty();
                let has_c = (!lp.ca_r.is_empty() || !photo.ca_r.is_empty())
                    && (!lp.ca_b.is_empty() || !photo.ca_b.is_empty());
                if has_v || has_d || has_c {
                    ui.label(tr(lang, K_DEVELOP_PROFILE_CORRECTIONS_FROM_CAMERA_METADATA));
                    let mut pch = false;
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(has_v, egui::Checkbox::new(&mut lp.vignette_on, tr(lang, K_DEVELOP_VIGNETTING)))
                            .on_hover_text(tr(lang, K_DEVELOP_THE_CAMERA_S_OWN_FALLOFF_MAP_FOR_THIS_SHOT_APPLI))
                            .changed()
                        {
                            if lp.vignette_on && lp.vignette.is_empty() {
                                lp.vignette = photo.vignette.clone();
                            }
                            pch = true;
                        }
                        if ui
                            .add_enabled(has_d, egui::Checkbox::new(&mut lp.distortion_on, tr(lang, K_DEVELOP_DISTORTION)))
                            .on_hover_text(tr(lang, K_DEVELOP_THE_CAMERA_S_OWN_GEOMETRIC_CORRECTION_MASKS_AND))
                            .changed()
                        {
                            if lp.distortion_on && lp.distortion.is_empty() {
                                lp.distortion = photo.distortion.clone();
                            }
                            pch = true;
                        }
                        if ui
                            .add_enabled(has_c, egui::Checkbox::new(&mut lp.ca_on, tr(lang, K_DEVELOP_CHROMATIC_ABERRATION)))
                            .on_hover_text(tr(lang, K_DEVELOP_PER_CHANNEL_RADIUS_CORRECTION_REMOVES_RED_BLUE_C))
                            .changed()
                        {
                            if lp.ca_on && (lp.ca_r.is_empty() || lp.ca_b.is_empty()) {
                                // Refill only the MISSING channel(s): `has_c`
                                // accepts the union of saved + photo data, so
                                // replacing BOTH from the photo could erase a
                                // saved channel the photo can't supply and
                                // leave the pair incomplete (CA that renders
                                // nothing — geometry_active needs both).
                                if lp.ca_r.is_empty() {
                                    lp.ca_r = photo.ca_r.clone();
                                }
                                if lp.ca_b.is_empty() {
                                    lp.ca_b = photo.ca_b.clone();
                                }
                            }
                            pch = true;
                        }
                    });
                    if pch {
                        self.workspace.overlay_mut().overlay_stale = true;
                        changed = true;
                    }
                    ui.add_space(SPACE_MD);
                } else if self.editor.photo().src_path.as_deref().is_some_and(autoshade::decode::is_raw) {
                    ui.label(
                        Text::new(tr(lang, K_DEVELOP_NO_IN_CAMERA_LENS_CORRECTION_DATA_IN_THIS_FILE))
                            .weak()
                            .small(),
                    );
                    ui.add_space(SPACE_MD);
                }
                // 「Lens vignetting」, not 「Vignette」 (R25 B2): the Effects
                // section above now carries Lightroom's POST-CROP vignette,
                // and two sections labelled 暗角 with different operators
                // behind them is a name collision, not a shorthand. This one
                // is a falloff CORRECTION in linear light before any tonal
                // work; that one is a creative darkening after the crop.
                // Manual vignetting / midpoint / distortion: one capability
                // table (bands and resets from `logic::capability`).
                changed |= self.capability_rows(ui, crate::controls::LENS_MANUAL);
                // --- manual lateral CA (R25 B3) -------------------------
                // RENDERED, so NO disclosure line: these two fold onto the
                // very per-channel radius LUT the profile CA above uses
                // (`render::geometry_profile`), and the preview, the export
                // and the sidecar all agree. The pair sits under the
                // distortion slider because it is the same kind of thing —
                // an optical defect, not a mood.
                // The PROFILE's own two strengths (v1.5.0). They belong with
                // the profile's tick boxes above, not with the manual sliders
                // below: they scale what the profile does, and at 0 they are
                // that component's off switch — which is why they are drawn
                // even when this photo has no profile data, so a recipe
                // imported from Lightroom shows what it carries.
                ui.add_space(SPACE_SM);
                ui.label(Text::new(tr(lang, K_DEVELOP_PROFILE_CORRECTION_STRENGTH)).weak().small());
                changed |= self.capability_rows(ui, crate::controls::LENS_PROFILE);
                ui.add_space(SPACE_SM);
                ui.label(Text::new(tr(lang, K_DEVELOP_CHROMATIC_ABERRATION_MANUAL)).weak().small());
                changed |= self.capability_rows(ui, crate::controls::LENS_CA);
                // …and the rest of the same panel: Adobe's auto switch and the
                // whole de-fringe block. Carried and unrendered until v1.5.0;
                // the switch is an INSTRUCTION, so its tooltip says what the
                // instruction does rather than what the number means.
                if ui
                    .checkbox(&mut self.editor.doc_mut().recipe.auto_lateral_ca, tr(lang, K_DEVELOP_AUTO_LATERAL_CA))
                    .on_hover_text(tr(
                        lang,
                        K_DEVELOP_MEASURES_THIS_FRAME_S_OWN_RED_BLUE_MISALIGNMENT,
                    ))
                    .changed()
                {
                    changed = true;
                }
                ui.add_space(SPACE_SM);
                ui.label(Text::new(tr(lang, K_DEVELOP_DEFRINGE)).weak().small());
                // Adobe's bands and neutrals: amounts rest at 0, the hue
                // windows at 30/70 and 40/60 (the one block whose neutral is
                // not zero — the row resets carry them).
                changed |= self.capability_rows(ui, crate::controls::LENS_DEFRINGE);
                ui.add_space(SPACE_SM);
                explainer(ui, lang, tr(lang, K_DEVELOP_VIGNETTE_POSITIVE_BRIGHTENS_THE_CORNERS_COMPENSA));
            });
        changed
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_curves(&mut self, ui: &mut egui::Ui, curves_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        ui.add_space(SPACE_MD);
        // --- 曲线: master + RGB tone curves (engine + XMP already apply them,
        // this is purely the editing surface — Lightroom's panel order) --------
        section_fold(ui, "sec_curves", section_title(tr(lang, K_DEVELOP_CURVES), curves_active), false, false, |ui| {
                changed |= self.curve_editor(ui);
                changed |= parametric_curve(ui, lang, &mut self.editor.doc_mut().recipe);
            });
        changed
    }
    /// What one Analyze run wrote — the verdict, the rationale sentence and
    /// the deep-thinking working — drawn by both the Analysis fold's bounded
    /// box and the full-text window, so the two can never disagree.
    pub(crate) fn analysis_output(&self, ui: &mut egui::Ui) {
        let lang = self.lang;
        if let Some((d, reasons)) = &self.editor.analysis().verdict {
            // Accept reads calm; anything else (Revise/Reject)
            // gets the warn colour so it can't be skimmed past.
            // Matched on the TYPED decision, never its rendered
            // spelling — the old `starts_with("Accept")` sniff
            // would have flipped every verdict to warn the moment
            // the word was translated.
            let col = if matches!(d, autoshade::advisor::Decision::Accept) {
                ui.visuals().strong_text_color()
            } else {
                ui.visuals().warn_fg_color
            };
            let text = trf(
                lang,
                K_AI_TEXT,
                &[
                    ("decision", tr(lang, autoshade::advisor::decision_key(d))),
                    ("reasons", &reasons.join("; ")),
                ],
            );
            ui.label(Text::new(text).color(col));
        }
        // The deterministic tail renders LOCALIZED when its typed
        // notes ride along and still match the string's suffix
        // (L12#2B); any mismatch — a truncation, a disk-restored
        // develop with no notes — shows the raw English instead
        // (silent-English fallback, user decision 2026-08-11).
        //
        // The typed tail splits in two at draw time (user feedback
        // 2026-09-11): the deterministic notes stay in the sentence, the
        // deep-thinking WORKING — the notes only a thinking analysis
        // produces, see `is_working_note` — draws in a box of its own
        // under it. The persisted string is untouched (one suffix, five
        // surfaces); the split is a display decision, and on the fallback
        // path the working is still inside `shown`, so no box is drawn.
        let (working, tail): (Vec<_>, Vec<_>) =
            self.editor.analysis().rationale_notes.iter().partition(|n| is_working_note(n.key));
        let localized = (!self.editor.analysis().rationale_notes.is_empty())
            .then(|| {
                let det: String = self.editor.analysis().rationale_notes
                    .iter()
                    .map(autoshade::rationale::render_one)
                    .collect();
                self.editor.analysis().rationale.strip_suffix(det.as_str()).map(|prose| {
                    let mut s = String::from(prose);
                    for n in &tail {
                        s.push_str(&trf(lang, n.key, &note_args(n)));
                    }
                    s
                })
            })
            .flatten();
        let shown = localized.as_deref().unwrap_or(&self.editor.analysis().rationale);
        if !shown.is_empty() {
            ui.label(
                Text::new(format!("“{shown}”"))
                    .italics()
                    .weak(),
            );
        }
        if localized.is_some() && !working.is_empty() {
            thinking_notes(self.lang, ui, &working);
        }
    }
    /// The colour-field row (the mask's own "one luminance colour" row; not a
    /// mask, which is why it is not a list entry).
    pub(crate) fn dev_mask_colour_field(&mut self, ui: &mut egui::Ui) -> bool {
        let lang = self.lang;
        let mut changed = false;
        // R33 §G. The colour field, in the Masks section because that is
        // where a photographer looks for "what else is local about this
        // photo" — one row, never a list: a recipe holds at most one.
        //
        // Its own row rather than a mask entry, because it is not a mask:
        // it has no geometry to select, no coverage to preview and no
        // stack position to drag. Three controls, which are the three
        // questions a user actually has about it — is it on, how much of
        // it, and get rid of it.
        if self.editor.doc().recipe.colour_field.is_some() {
            ui.add_space(SPACE_XS);
            let mut remove_field = false;
            ui.horizontal(|ui| {
                let field = self.editor.doc_mut().recipe.colour_field.as_mut().expect("just checked");
                if glyph_toggle(ui, field.enabled, "👁")
                    .on_hover_text(tr(
                        lang,
                        K_DEVELOP_SHOW_MUTE_THE_COLOUR_FIELD_WITHOUT_LOSING_ITS_AM,
                    ))
                    .clicked()
                {
                    field.enabled = !field.enabled;
                    changed = true;
                }
                let label = format!(
                    "▦ {} · {}",
                    tr(lang, K_DEVELOP_COLOUR_FIELD),
                    tr(lang, K_DEVELOP_ENGINE_ONLY),
                );
                ui.label(if field.enabled {
                    Text::new(label)
                } else {
                    Text::new(label).weak()
                })
                .on_hover_text(tr(
                    lang,
                    K_DEVELOP_A_SMOOTH_LOCAL_COLOUR_TONE_FIELD_THE_REVERSE_FIT,
                ));
                if glyph(ui, true, "🗑")
                    .on_hover_text(tr(lang, K_DEVELOP_REMOVE_THE_COLOUR_FIELD))
                    .clicked()
                {
                    remove_field = true;
                }
            });
            let field = self.editor.doc_mut().recipe.colour_field.as_mut().expect("just checked");
            // Through the shared slider (its look, reset to the solved
            // field's full amount, ↑/↓) — it was the one bare egui slider.
            let enabled = field.enabled;
            if ui
                .add_enabled_ui(enabled, |ui| {
                    slider_pct(ui, lang, tr(lang, K_DEVELOP_AMOUNT), &mut field.amount, 1.0, 1.0)
                })
                .inner
            {
                changed = true;
            }
            if remove_field {
                self.editor.doc_mut().recipe.colour_field = None;
                changed = true;
            }
        }
        changed
    }

    /// The selected mask's adjustment half: the four captioned groups and the
    /// mask's own curve editor. Tone edits are not coverage, so they are
    /// deliberately outside the overlay invalidation above.
    pub(crate) fn dev_mask_adjustments(&mut self, ui: &mut egui::Ui, i: usize) -> bool {
        let lang = self.lang;
        let mut changed = false;
        let m = &mut self.editor.doc_mut().recipe.masks[i];
        // Lightroom's own three groups for a mask's adjustments (user
        // decision 8 of the 17-feedback triage): Tone / Detail /
        // Color, weak captions in the Color-Grading "All regions"
        // style. Twelve sliders in one unbroken column is what the
        // grouping replaces — the same reason the global panel is
        // sectioned. `Tone` is our own caption for LR's "Light" group:
        // the key "Light" already means the light THEME.
        let group = |ui: &mut egui::Ui, title: &str| sub_caption(ui, title);
        group(ui, tr(lang, K_DEVELOP_TONE));
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_EXPOSURE), &mut m.exposure_ev, -5.0, 5.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_CONTRAST), &mut m.contrast, -100.0, 100.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_HIGHLIGHTS), &mut m.highlights, -100.0, 100.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_SHADOWS), &mut m.shadows, -100.0, 100.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_WHITES), &mut m.whites, -100.0, 100.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_BLACKS), &mut m.blacks, -100.0, 100.0, 0.0);
        // Texture / Clarity / Dehaze come out of the old
        // 「More (XMP/Lightroom only)」 fold: R22-3 put all three ON the
        // engine (render::apply_masks), so that title became a lie and
        // the fold merely hid three working sliders one level deep.
        group(ui, tr(lang, K_DEVELOP_DETAIL));
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_TEXTURE), &mut m.texture, -100.0, 100.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_CLARITY), &mut m.clarity, -100.0, 100.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_DEHAZE), &mut m.dehaze, -100.0, 100.0, 0.0);
        // R23-1b. SIGNED, unlike the global 0..150 Sharpening: this is
        // ACR's local band, and its negative half (soften) is what
        // throws a background back — the tooltip says so, because the
        // two controls share a name and not a range.
        changed |= slider_hinted(
            ui,
            lang,
            tr(lang, K_DEVELOP_SHARPNESS),
            &mut m.sharpness,
            -100.0,
            100.0,
            0.0,
            tr(lang, K_DEVELOP_SHARPENS_INSIDE_THE_MASK_WHEN_POSITIVE_AND_SOFTE),
        );
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_NOISE_REDUCTION), &mut m.noise_reduction, 0.0, 100.0, 0.0);
        group(ui, tr(lang, K_DEVELOP_COLOR));
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_SATURATION), &mut m.saturation, -100.0, 100.0, 0.0);
        // R23-1b: a hue ROTATION of everything the mask covers — the
        // one colour move the global 8-band mixer cannot make, because
        // that one acts on a band everywhere in the frame.
        changed |= slider_hinted(
            ui,
            lang,
            tr(lang, K_DEVELOP_HUE_SHIFT),
            &mut m.hue,
            -100.0,
            100.0,
            0.0,
            tr(lang, K_DEVELOP_ROTATES_EVERY_COLOUR_INSIDE_THE_MASK_100_30_UNLI),
        );
        // Engine-rendered since batch #2-B (render.rs apply_masks
        // mirrors the global WB model inside the mask) — live in the
        // preview like the tone sliders above. The ±100 pair is a
        // RELATIVE gel around a fixed anchor, a different axis from
        // the global 「Temp (K)」 absolute Kelvin — two controls named
        // "temp" that answer different questions, so the tooltip
        // carries the engine's OWN conversion (render::
        // local_temp_to_kelvin, not a number retyped here) for the
        // value currently on the slider.
        let temp_k = format!(
            "{:.0}",
            autoshade::render::local_temp_to_kelvin(m.temperature)
        );
        let temp_hint = trf(
            lang,
            K_DEVELOP_A_RELATIVE_WARM_COOL_SHIFT_100_AROUND_A_FIXED_55,
            &[("k", &temp_k)],
        );
        changed |= slider_hinted(ui, lang, tr(lang, K_DEVELOP_TEMP_SHIFT), &mut m.temperature, -100.0, 100.0, 0.0, &temp_hint);
        changed |= slider_hinted(
            ui,
            lang,
            tr(lang, K_DEVELOP_TINT_SHIFT),
            &mut m.tint,
            -100.0,
            100.0,
            0.0,
            tr(lang, K_DEVELOP_A_RELATIVE_GREEN_MAGENTA_SHIFT_100_INSIDE_THE_MA),
        );
        // Lightroom's mask "Color" swatch (`crs:LocalToningSaturation`
        // / `LocalToningHue`), as its two numbers: how strongly, then
        // toward which hue. Last in the group because the render
        // applies it last (`render::apply_masks`); engine-rendered and
        // carried by the sidecar, and the hue does nothing at 0.
        changed |= slider_hinted(
            ui,
            lang,
            tr(lang, K_DEVELOP_COLOR_TINT),
            &mut m.toning_amount,
            0.0,
            100.0,
            0.0,
            tr(lang, K_DEVELOP_LIGHTROOM_S_MASK_COLOR_SWATCH_SHIFTS_EVERY_COLOU),
        );
        changed |= slider_hinted(
            ui,
            lang,
            tr(lang, K_DEVELOP_COLOR_TINT_HUE),
            &mut m.toning_hue,
            0.0,
            360.0,
            0.0,
            tr(lang, K_DEVELOP_THE_HUE_0_360_COLOR_TINT_PUSHES_TOWARD_ON_LIGHTR),
        );
        // Reverse-fit recolour gains: per-channel linear gains no
        // classic-ACR key can express (recipe.rs `color_gains`); the
        // sidecar carries them as the mask's Temp/Tint through Camera
        // Raw's mask white law (`render::gains_as_white`). Not a
        // slider — a disclosure plus the one action that makes sense
        // on a value the user never typed.
        if m.color_gains.is_some() {
            ui.horizontal_wrapped(|ui| {
                // One cell of the grid, measured at the row's start;
                // it wraps under the note when the note is long (2026-09-27).
                let cell = columns(ui, 2);
                ui.label(
                    Text::new(tr(lang, K_DEVELOP_CARRIES_REVERSE_FIT_RECOLOUR_WRITTEN_TO_XMP_AS_T))
                        .weak()
                        .small(),
                );
                if action_in(ui, cell, true, tr(lang, K_DEVELOP_CLEAR))
                    .on_hover_text(tr(lang, K_DEVELOP_DROP_THIS_MASK_S_PER_CHANNEL_RECOLOUR_GAINS_ONE))
                    .clicked()
                {
                    m.color_gains = None;
                    changed = true;
                }
            });
        }
        // R25 P6 — the FOURTH group under Lightroom's three: this
        // mask's own point curves (`crs:MainCurve` / `RedCurve` /
        // `GreenCurve` / `BlueCurve`). The very editor the global
        // Curves section uses, pointed at this mask, so the plot, the
        // click/drag/drag-out gestures and the engine-faithful line
        // stay ONE implementation — a second copy is where the
        // preview line drifts from `render::curve_lut`.
        //
        // Last in the section on purpose: it is the only control here
        // that is not a slider, and it is the tallest.
        group(ui, tr(lang, K_DEVELOP_CURVE));
        changed |= self.curve_editor_for(ui, CurveTarget::Mask(i));
        changed
    }
    /// One capability table through the registry (the panel's only write
    /// surface for sliders).
    pub(crate) fn capability_rows(
        &mut self,
        ui: &mut egui::Ui,
        table: &[&dyn crate::controls::ControlWidget],
    ) -> bool {
        crate::panels::develop_forms::render_and_apply(ui, self.lang, &mut self.editor.doc_mut().recipe, table)
    }

    /// Report a batch of intents.
    pub(crate) fn act_all(&mut self, actions: impl IntoIterator<Item = logic::Action>) {
        self.actions.extend(actions);
    }

    /// Toggle the clipping overlay (the J overlay and the histogram's corner
    /// triangles share it). At rest it drops the layer; once on it rebuilds
    /// from the retained RGB frame, so the toggle is instant — no redevelop.
    pub(crate) fn toggle_clipping(&mut self, ctx: &egui::Context) {
        self.workspace.chrome_mut().show_clipping = !self.workspace.chrome().show_clipping;
        if !self.workspace.chrome().show_clipping {
            self.workspace.chrome_mut().clip_tex = None; // OFF is just dropping the layer
            return;
        }
        match &self.workspace.chrome().last_rgb {
            Some(rgb) => {
                let clip = clipping_overlay_for(rgb, self.editor.doc().recipe.crop);
                if let Some(tex) = &mut self.workspace.chrome_mut().clip_tex {
                    tex.set(clip, egui::TextureOptions::NEAREST);
                } else {
                    self.workspace.chrome_mut().clip_tex =
                        Some(ctx.load_texture("clip", clip, egui::TextureOptions::NEAREST));
                }
            }
            None => self.editor.session_mut().dirty = true, // nothing retained (fresh open) — worker builds it
        }
    }
}
