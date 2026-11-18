//! Gallery panel: folder thumbnails and selection.

use crate::*;

impl PanelView<'_, '_> {
    /// Left-most panel: the working-folder thumbnail gallery. Only visible rows
    /// are laid out (show_rows) and only their thumbnails are queued to decode.
    pub(crate) fn gallery_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        // Title and its one verb share a line when the panel is wide enough
        // and WRAP otherwise. A plain horizontal let the CJK title ("图库 ·
        // Library") plus the verb outgrow the 240 px default by ~1 px, and a
        // side panel sizes itself to its widest row — every row below then
        // lost its right edge. Wrapping keeps the panel at its default
        // whatever the font metrics do.
        ui.row_wrapped(|ui| {
            ui.heading(tr(lang, K_GALLERY_LIBRARY));
            if action(ui, true, tr(lang, K_APP_OPEN_FOLDER)).clicked() {
                self.act_all(vec![logic::Action::Gallery(
                    logic::gallery::GalleryAction::OpenFolder,
                )]);
            }
        });
        if let Some(d) = &self.workspace.gal().gallery_dir {
            let dir = d.display().to_string();
            let cnt = self.workspace.gal().gallery.len().to_string();
            ui.label(
                Text::new(trf(lang, K_GALLERY_PHOTOS, &[("dir", &dir), ("count", &cnt)]))
                    .weak()
                    .small(),
            );
        }
        // Batch: copy the open photo's recipe → Ctrl+click a selection → paste.
        // Lightroom's "sync settings" for the whole working folder. R38: three
        // full-width rows — at the 240 px default the three verbs never shared
        // a line anyway, and a scoped button after a wrapped line's end was
        // laid past the panel edge (a scope never wraps); ✕ shares the last
        // row while there is a selection to clear.
        let cell = columns(ui, 1);
        // `!busy` like both siblings below. `open_path` re-points
        // `src_path` immediately but `recipe` only refreshes when the
        // decode lands, so during an open this pairs photo A's recipe
        // (below) with photo B's path — and `copied_from` is what
        // suppresses the "bitmap mask(s) not pasted" warning, so the
        // mismatch INVERTS that guard and writes A's raster masks into
        // B's saved develop.
        if action_in(ui, cell, self.editor.photo().src_path.is_some() && !self.workspace.busy(), tr(lang, K_GALLERY_COPY_RECIPE))
            .on_hover_text(tr(lang, K_GALLERY_COPY_EVERY_DEVELOP_SETTING_FROM_THE_CURRENT_PHOT))
            .clicked()
        {
            self.act_all(vec![logic::Action::Gallery(logic::gallery::GalleryAction::CopyRecipe)]);
        }
        let n = self.workspace.gal().multi_sel.len();
        let n_s = n.to_string();
        if action_in(ui, cell, self.workspace.genai().copied.is_some() && n > 0 && !self.workspace.busy(), trf(lang, K_GALLERY_PASTE_TO_SELECTED, &[("n", &n_s)]))
            .on_hover_text(tr(lang, K_GALLERY_WRITES_EACH_PHOTO_S_DEVELOP_INTO_YOUR_DEVELOP_ST))
            .clicked()
        {
            self.act_all(vec![logic::Action::Gallery(
                logic::gallery::GalleryAction::PasteToSelected,
            )]);
        }
        ui.row(|ui| {
            // The last row lends ✕ (clear the selection) one square of its
            // width while there is a selection to clear.
            let wide = if n > 0 {
                egui::vec2(cell.x - row_h(ui) - ui.spacing().item_spacing.x, cell.y)
            } else {
                cell
            };
            if action_in(ui, wide, n > 0 && !self.workspace.busy(), trf(lang, K_GALLERY_RENDER_SELECTED, &[("n", &n_s)]))
                .on_hover_text(tr(
                    lang,
                    K_GALLERY_EACH_RENDERS_BY_ITS_OWN_SAVED_DEVELOP_FROM_THE_S,
                ))
                .clicked()
            {
                self.act_all(vec![logic::Action::Gallery(
                    logic::gallery::GalleryAction::RenderSelected,
                )]);
            }
            if n > 0 && glyph(ui, true, "✕").on_hover_text(tr(lang, K_GALLERY_CLEAR_SELECTION)).clicked() {
                self.act_all(vec![logic::Action::Gallery(
                    logic::gallery::GalleryAction::ClearSelection,
                )]);
            }
        });
        if self.workspace.genai().copied.is_some() {
            ui.checkbox(&mut self.workspace.genai_mut().paste_geometry, tr(lang, K_GALLERY_INCLUDE_CROP_STRAIGHTEN_WHEN_PASTING))
                .on_hover_text(tr(lang, K_GALLERY_OFF_BY_DEFAULT_COMPOSITION_RARELY_TRANSFERS_BETW));
        }
        ui.rule();
        if action_row(ui, true, tr(lang, K_GALLERY_RECIPE_LIBRARY))
            .on_hover_text(tr(
                lang,
                K_GALLERY_SAVE_THIS_PHOTO_S_LOOK_UNDER_A_NAME_APPLY_IT_TO,
            ))
            .clicked()
        {
            self.act_all(vec![logic::Action::Gallery(
                logic::gallery::GalleryAction::OpenRecipeLibrary,
            )]);
        }
        ui.rule();
        if self.workspace.gal().gallery.is_empty() {
            // Two different situations, two different messages: "no folder"
            // invites opening one; "empty folder" says the folder itself has
            // nothing to show (the old shared line read as a broken open).
            let hint = if self.workspace.gal().gallery_dir.is_some() {
                tr(lang, K_GALLERY_NO_PHOTOS_IN_THIS_FOLDER_RAW_JPEG_PNG_TIFF_WOULD)
            } else {
                tr(lang, K_GALLERY_OPEN_A_FOLDER_TO_BROWSE_YOUR_PHOTOS_HERE)
            };
            ui.gap(SPACE_MD);
            ui.label(Text::new(hint).weak());
            return;
        }

        let count = self.workspace.gal().gallery.len();
        // Borrow only what the row closure reads, from ONE gal borrow; collect
        // actions to apply after (a thumb request and an open both need
        // &mut self).
        let colors = self.workspace.ui().theme.colors();
        // The scroll target is taken BEFORE the row scope borrows `gal`.
        let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(i) = self.workspace.gal_mut().gallery_scroll_to.take() {
            // Keyboard navigation moved the selection: jump the viewport so the
            // highlighted row sits roughly centred. show_rows only lays out
            // visible rows, so scroll_to_me can never reach an off-screen row —
            // the offset (rows are fixed-height) is the reliable route.
            let target = (i as f32 * GALLERY_ROW_H - ui.available_height() * 0.4).max(0.0);
            scroll = scroll.vertical_scroll_offset(target);
        }
        let gal = self.workspace.gal_mut();
        let thumbs = &gal.thumbs;
        let edited_badge = &mut gal.edited_badge;
        let gallery = &gal.gallery;
        let selected = gal.selected;
        let multi_sel = &gal.multi_sel;
        // A row click is an action; the decision (open vs toggle) is the
        // logic layer's, applied after the row closure ends.
        let mut clicked: Option<(usize, bool)> = None;
        let mut to_request: Vec<usize> = Vec::new();
        let mut visible: std::ops::Range<usize> = 0..0;

        // Geometry of the first drawn thumbnail this frame (slot, image) —
        // recorded through a local because the row closure below borrows
        // `self`'s fields piecemeal and cannot touch `self` itself.
        #[cfg(test)]
        let mut thumb_geom: Option<(egui::Rect, egui::Rect)> = None;
        scroll
            .show_rows(ui, GALLERY_ROW_H, count, |ui, range| {
                visible = range.clone();
                for i in range {
                    let path = &gallery[i];
                    let is_sel = selected == Some(i);
                    let is_multi = multi_sel.contains(&i);
                    let fill = if is_sel {
                        colors.sel_bg
                    } else if is_multi {
                        colors.sel_bg_dim
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    let resp = egui::Frame::NONE
                        .fill(fill)
                        .corner_radius(egui::CornerRadius::same(RADIUS_SM.round() as u8)) // selected-row fill follows the thumb radius
                        .inner_margin(egui::Margin::same(3))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.avail_width());
                            ui.row(|ui| {
                                if let Some(t) = thumbs.get(&i) {
                                    // CONSTANT SLOT + aspect-preserving inset
                                    // (#17). The old code handed egui a
                                    // SizedTexture claiming the texture WAS
                                    // THUMB_W × THUMB_H; a Texture source is
                                    // drawn at ImageFit::Exact, so that lie
                                    // squashed every portrait into the same
                                    // 1.4:1 landscape box (the decode-side
                                    // orientation chain was never at fault).
                                    // The slot stays fixed — byte-identical to
                                    // the placeholder branch below — because it
                                    // is what keeps the filename column aligned
                                    // down the whole list; a portrait gets a
                                    // ~27 × 40 letterboxed inset inside it.
                                    let (slot, _) = ui.allocate_exact_size(
                                        egui::vec2(THUMB_W, THUMB_H),
                                        egui::Sense::hover(),
                                    );
                                    // Thumbnails decode to THUMB_EDGE (160 px)
                                    // long edge, so the scale is ≤ 0.35 here and
                                    // fit_in's 4× upscale cap never bites.
                                    let draw = fit_in(t.size_vec2(), THUMB_W, THUMB_H);
                                    let rect = egui::Rect::from_center_size(slot.center(), draw);
                                    // painter().image() has no rounding, and the
                                    // rounded corners are the gallery's look —
                                    // so this is egui's own paint_texture_at
                                    // shape (widgets/image.rs), inlined.
                                    ui.painter().add(egui::epaint::RectShape {
                                        rect,
                                        corner_radius: egui::CornerRadius::same(RADIUS_SM.round() as u8),
                                        fill: egui::Color32::WHITE, // tint: the texture unmodified
                                        stroke: egui::Stroke::NONE,
                                        stroke_kind: egui::StrokeKind::Inside,
                                        round_to_pixels: None,
                                        blur_width: 0.0,
                                        brush: None,
                                    }.with_texture(
                                        t.id(),
                                        egui::Rect::from_min_max(
                                            egui::pos2(0.0, 0.0),
                                            egui::pos2(1.0, 1.0),
                                        ),
                                    ));
                                    #[cfg(test)]
                                    {
                                        if thumb_geom.is_none() {
                                            thumb_geom = Some((slot, rect));
                                        }
                                    }
                                } else {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(THUMB_W, THUMB_H),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(rect, RADIUS_SM, colors.thumb_placeholder);
                                    to_request.push(i);
                                }
                                ui.column(|ui| {
                                    let mut name = Text::new(autoshade::pipeline::stem(path)).small();
                                    if is_sel {
                                        name = name.strong().color(colors.accent_text);
                                    }
                                    // Truncate, never wrap: show_rows assumes
                                    // every row is exactly GALLERY_ROW_H, and a
                                    // wrapped long filename would misalign the
                                    // whole virtualized scroll.
                                    ui.add(egui::Label::new(name).truncate());
                                    // The badge is a fact about the photo's
                                    // store/sidecar, cached per row by the
                                    // logic layer (this loop runs per repaint),
                                    // and dropped whenever this app writes a
                                    // sidecar or the folder changes.
                                    let edited = logic::gallery::edited_badge(edited_badge, i, path);
                                    let baked = !autoshade::decode::is_raw(path);
                                    ui.row(|ui| {
                                        if is_multi {
                                            ui.label(Text::new(tr(lang, K_GALLERY_SELECTED)).color(colors.accent_text).small());
                                        }
                                        if baked {
                                            ui.label(Text::new("PNG/TIFF").color(colors.accent_text).small());
                                        }
                                        if edited {
                                            ui.label(Text::new(tr(lang, K_GALLERY_EDITED)).color(colors.accent_text).small());
                                        }
                                    });
                                });
                            });
                        })
                        .response
                        .interact(egui::Sense::click());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        clicked = Some((i, ui.input(|inp| inp.modifiers.command)));
                    }
                }
            });

        #[cfg(test)]
        {
            if let Some((slot, draw)) = thumb_geom {
                self.workspace.gal_mut().gallery_slot_rect = Some(slot);
                self.workspace.gal_mut().gallery_thumb_rect = Some(draw);
            }
        }
        if !to_request.is_empty() {
            let asked: Vec<logic::Action> = to_request
                .into_iter()
                .map(|i| logic::Action::Gallery(logic::gallery::GalleryAction::ThumbNeeded(i)))
                .collect();
            self.act_all(asked);
        }
        // Bounded GPU memory: the window is the logic layer's, the retains
        // are the runtime's (`GalleryAction::Trim`).
        if let Some(keep) =
            logic::gallery::trim_window(visible.clone(), self.workspace.gal().thumbs.len(), logic::gallery::THUMB_TEX_CAP)
        {
            self.act_all(vec![logic::Action::Gallery(
                logic::gallery::GalleryAction::Trim { start: keep.start, end: keep.end },
            )]);
        }
        if let Some((index, multi)) = clicked {
            self.act_all(vec![logic::Action::Gallery(
                logic::gallery::GalleryAction::Click { index, multi },
            )]);
        }
    }
}
