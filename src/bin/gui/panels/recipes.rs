//! The recipe library window: named, portable develops kept outside any one
//! photo (`autoshade::recipe_library`), applied through the same paths the
//! clipboard paste uses.

use crate::*;

impl Commands<'_> {
    /// Open the window (the Library panel's 「Recipe library…」 button).
    pub(crate) fn open_recipe_library(&mut self) {
        self.workspace.rlib_mut().show_recipes = true;
        self.apply_recipe(logic::recipes::RecipeAction::Refresh);
    }

    /// One `recipe_library` window frame.
    pub(crate) fn recipe_library_window(&mut self, ctx: &egui::Context) {
        if !self.workspace.rlib().show_recipes {
            return;
        }
        if !self.workspace.rlib().recipes_loaded {
            self.workspace.refresh_recipes();
        }
        let lang = self.lang;
        let mut open = true;
        egui::Window::new(tr(lang, K_RECIPES_RECIPE_LIBRARY))
            // Fixed id: the title varies with the language and egui keys
            // window state (position, size) off the id.
            .id(egui::Id::new("recipe_library_window"))
            .collapsible(false)
            .default_width(520.0)
            .open(&mut open)
            .show(ctx, |ui| self.recipe_library_ui(ui));
        if !open {
            self.workspace.rlib_mut().show_recipes = false;
        }
    }

    fn recipe_library_ui(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let colors = self.workspace.ui().theme.colors();
        let n = self.workspace.rlib().recipes.len();
        ui.set_min_width(540.0);

        // --- new recipe ------------------------------------------------------
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr(lang, K_RECIPES_NEW_RECIPE)).weak().small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if action(ui, true, tr(lang, K_RECIPES_IMPORT_FILE))
                    .on_hover_text(tr(lang, K_RECIPES_ADD_A_RECIPE_JSON_OR_A_LIGHTROOM_CAMERA_RAW_XMP))
                    .clicked()
                {
                    self.apply_recipe(logic::recipes::RecipeAction::ImportFile,);
                }
            });
        });
        ui.horizontal(|ui| {
            let ready = self.editor.photo().src_path.is_some() && !self.workspace.busy();
            let button_w = 92.0;
            let field_w = (ui.available_width() - button_w - ui.spacing().item_spacing.x).max(180.0);
            let resp = ui.add_sized(
                [field_w, row_h(ui)],
                egui::TextEdit::singleline(&mut self.workspace.rlib_mut().recipe_name_buf).hint_text(tr(lang, K_RECIPES_RECIPE_NAME)),
            );
            // Enter in the name field saves, like every other name box in the
            // app (the U10 boundary rule): a typed name plus Enter is the
            // shortest path from "this look" to "a named look".
            let entered = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let clicked = primary_in(ui, egui::vec2(button_w, row_h(ui)), ready, tr(lang, K_RECIPES_SAVE))
                .on_hover_text(tr(
                    lang,
                    K_RECIPES_SAVES_THIS_PHOTO_S_LOOK_UNDER_THE_NAME_IN_THE_FI,
                ))
                .on_disabled_hover_text(tr(lang, K_RECIPES_OPEN_A_PHOTO_FIRST_THIS_SAVES_THE_OPEN_PHOTO_S_L))
                .clicked();
            if ready && (clicked || entered) {
                self.apply_recipe(logic::recipes::RecipeAction::SaveCurrent,);
            }
        });
        ui.separator();

        // --- filter ----------------------------------------------------------
        let filter_empty = self.workspace.rlib().recipes_filter.trim().is_empty();
        ui.horizontal(|ui| {
            let clear_w = if filter_empty { 0.0 } else { row_h(ui) + ui.spacing().item_spacing.x };
            let count_w = 80.0;
            let w = (ui.available_width() - clear_w - count_w - ui.spacing().item_spacing.x).max(140.0);
            ui.add_sized(
                [w, row_h(ui)],
                egui::TextEdit::singleline(&mut self.workspace.rlib_mut().recipes_filter).hint_text(tr(lang, K_RECIPES_FILTER)),
            );
            if !filter_empty
                && glyph(ui, true, "✕").on_hover_text(tr(lang, K_RECIPES_CLEAR_FILTER)).clicked()
            {
                self.workspace.rlib_mut().recipes_filter.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(trf(lang, K_RECIPES_SAVED, &[("n", &n.to_string())])).weak().small(),
                );
            });
        });

        // --- list ------------------------------------------------------------
        let filter = self.workspace.rlib().recipes_filter.trim().to_lowercase();
        let rows: Vec<(String, Option<String>, usize, Option<String>)> = self.workspace.rlib().recipes
            .iter()
            .filter(|r| filter.is_empty() || r.name.to_lowercase().contains(&filter))
            .map(|r| {
                (
                    r.name.clone(),
                    r.modified.and_then(|t| age_text(lang, t)),
                    r.masks,
                    r.error.clone(),
                )
            })
            .collect();
        let mut picked: Option<String> = None;
        let mut commit_rename: Option<(String, String)> = None;
        ui.add_space(SPACE_XS);
        egui::ScrollArea::vertical().id_salt("recipe_list").max_height(260.0).show(ui, |ui| {
            if rows.is_empty() {
                ui.add_space(SPACE_LG);
                let (headline, hint) = if self.workspace.rlib().recipes.is_empty() {
                    (
                        tr(lang, K_RECIPES_NO_RECIPES_YET),
                        tr(lang, K_RECIPES_SAVE_THIS_PHOTO_S_LOOK_OR_IMPORT_A_RECIPE_JSON_X),
                    )
                } else {
                    (
                        tr(lang, K_RECIPES_NO_RECIPE_MATCHES_THIS_FILTER),
                        tr(lang, K_RECIPES_CLEAR_THE_FILTER_TO_SEE_THE_WHOLE_LIBRARY),
                    )
                };
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("🖼").size(22.0).weak());
                    ui.add_space(SPACE_XS);
                    ui.label(egui::RichText::new(headline).weak());
                    ui.label(egui::RichText::new(hint).weak().small());
                });
                ui.add_space(SPACE_LG);
            }
            for (name, age, masks, error) in &rows {
                let selected = self.workspace.rlib().recipe_sel.as_deref() == Some(name.as_str());
                let editing = self.workspace.rlib().recipe_rename.as_ref().is_some_and(|(n, _)| n == name);
                if editing {
                    let mut buf = self.workspace.rlib().recipe_rename.as_ref().map(|(_, b)| b.clone()).unwrap_or_default();
                    let resp = ui.add_sized(
                        [ui.available_width(), RECIPE_ROW_H],
                        egui::TextEdit::singleline(&mut buf)
                            .id(egui::Id::new(("recipe_rename", name))),
                    );
                    if let Some((_, b)) = self.workspace.rlib_mut().recipe_rename.as_mut() {
                        *b = buf;
                    }
                    if resp.lost_focus() {
                        commit_rename = self.workspace.rlib_mut().recipe_rename.take();
                    } else if !resp.has_focus() {
                        // The frame the box first appears in: put the caret
                        // here or the user's next keystroke goes nowhere
                        // (the version-rename rule).
                        resp.request_focus();
                    }
                    continue;
                }
                // The meta sits at the right edge; a file that does not parse
                // says so there in the error colour instead of hiding.
                let meta = match error {
                    Some(_) => tr(lang, K_RECIPES_UNREADABLE).to_string(),
                    None => {
                        let mut meta = age.clone().unwrap_or_default();
                        if *masks == 1 {
                            meta = format!("{} · {}", meta, tr(lang, K_RECIPES_1_MASK));
                        } else if *masks > 1 {
                            meta = format!(
                                "{} · {}",
                                meta,
                                trf(lang, K_RECIPES_MASKS, &[("n", &masks.to_string())])
                            );
                        }
                        meta
                    }
                };
                let (resp, truncated) = paint_recipe_row(ui, colors, name, &meta, selected, error.is_some());
                // The full name only when the row had to cut it; the error
                // only when the file did not parse. A tooltip on every row
                // was a sentence following the pointer across the list.
                let resp = match (error, truncated) {
                    (Some(err), _) => resp.on_hover_text(trf(lang, K_RECIPES_UNREADABLE__2, &[("err", err)])),
                    (None, true) => resp.on_hover_text(name.as_str()),
                    (None, false) => resp,
                };
                if resp.clicked() {
                    picked = Some(name.clone());
                }
            }
        });
        if let Some((from, to)) = commit_rename {
            self.apply_recipe(logic::recipes::RecipeAction::Rename {
                from: from.clone(),
                to: to.clone(),
            });
        }
        if let Some(name) = picked {
            self.workspace.rlib_mut().recipe_sel = Some(name);
            self.workspace.rlib_mut().recipe_delete_armed = None;
        }
        ui.separator();
        // The selected entry: what it looks like on this photo, what it
        // changes, and the verbs that act on IT (the library-wide verbs —
        // paste onto the selection, open the folder — stay below).
        if let Some(name) = self.workspace.rlib().recipe_sel.clone() {
            self.recipe_details_pane(ui, &name);
            ui.separator();
        }

        // --- the library-wide verbs ------------------------------------------
        let sel = self.workspace.rlib().recipe_sel.clone();
        let targets = self.workspace.gal().multi_sel.len();
        let busy = self.workspace.busy();
        ui.horizontal_wrapped(|ui| {
            if action(
                ui,
                sel.is_some() && targets > 0 && !busy,
                trf(lang, K_RECIPES_APPLY_TO_SELECTED, &[("n", &targets.to_string())]),
            )
            .on_hover_text(tr(
                lang,
                K_GALLERY_WRITES_EACH_PHOTO_S_DEVELOP_INTO_YOUR_DEVELOP_ST,
            ))
            .on_disabled_hover_text(if targets == 0 {
                tr(lang, K_RECIPES_CTRL_CLICK_PHOTOS_IN_THE_LIBRARY_FIRST)
            } else {
                tr(lang, K_RECIPES_A_TASK_IS_RUNNING_WAIT_FOR_IT_TO_FINISH)
            })
            .clicked()
                && let Some(name) = &sel
            {
                self.apply_recipe(logic::recipes::RecipeAction::ApplyToSelection(name.to_owned()),);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if action(ui, true, tr(lang, K_RECIPES_SHOW_LIBRARY_FOLDER))
                    .on_hover_text(tr(lang, K_RECIPES_OPENS_THE_FOLDER_THAT_HOLDS_THESE_RECIPE_FILES))
                    .clicked()
                    && let Err(e) = reveal_folder(&autoshade::recipe_library::library_dir())
                {
                    self.workspace.recipe_status(
                        false,
                        trf(lang, K_RECIPES_THE_LIBRARY_FOLDER_COULD_NOT_BE_OPENED, &[("err", &e.to_string())]),
                    );
                }
            });
        });
    }

    /// The selected entry: its look developed on the open photo, the settings
    /// it actually changes, and the verbs that act on it.
    fn recipe_details_pane(&mut self, ui: &mut egui::Ui, name: &str) {
        let lang = self.lang;
        let entry = autoshade::recipe_library::read(name);
        let colors = self.workspace.ui().theme.colors();
        let photo_stem = self.editor.photo().src_path.as_deref().map(|p| autoshade::pipeline::stem(p).to_string());
        ui.horizontal_top(|ui| {
            const PREVIEW_W: f32 = 200.0;
            const PREVIEW_H: f32 = 140.0;
            let ctx = ui.ctx().clone();
            let tex = if entry.is_ok() { self.with_panel(&ctx, |p| p.recipe_preview_tex(&ctx, name)) } else { None };
            ui.vertical(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(PREVIEW_W, PREVIEW_H), egui::Sense::hover());
                match tex {
                    Some((id, size)) => {
                        let scale = (PREVIEW_W / size.x).min(PREVIEW_H / size.y).min(1.0);
                        let shown = size * scale;
                        let at = egui::Rect::from_center_size(rect.center(), shown);
                        egui::Image::new(egui::load::SizedTexture::new(id, shown)).paint_at(ui, at);
                        ui.painter().rect_stroke(
                            at.expand(1.0),
                            RADIUS_SM,
                            egui::Stroke::new(1.0, colors.hairline),
                            egui::StrokeKind::Middle,
                        );
                        if let Some(stem) = &photo_stem {
                            ui.label(
                                egui::RichText::new(trf(lang, K_RECIPES_ON, &[("photo", stem)]))
                                    .weak()
                                    .small(),
                            );
                        }
                    }
                    None => {
                        ui.painter().rect_filled(rect, RADIUS_MD, colors.canvas_bed);
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            tr(lang, K_RECIPES_OPEN_A_PHOTO_TO_SEE_THIS_RECIPE_ON_IT),
                            egui::TextStyle::Small.resolve(ui.style()),
                            ui.visuals().weak_text_color(),
                        );
                    }
                }
            });
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(name).strong());
                match &entry {
                    Ok(r) => {
                        let facts = recipe_facts(self.lang, r);
                        if facts.is_empty() {
                            ui.label(
                                egui::RichText::new(tr(lang, K_RECIPES_NO_EDITS_THIS_RECIPE_IS_NEUTRAL)).weak().small(),
                            );
                        } else {
                            egui::Grid::new("recipe_facts")
                                .num_columns(2)
                                .spacing([16.0, 3.0])
                                .show(ui, |ui| {
                                    for (label, value) in facts {
                                        ui.label(egui::RichText::new(label).weak().small());
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(egui::RichText::new(value).small());
                                            },
                                        );
                                        ui.end_row();
                                    }
                                });
                        }
                    }
                    Err(e) => {
                        ui.label(
                            egui::RichText::new(trf(lang, K_RECIPES_UNREADABLE__2, &[("err", &e.to_string())]))
                                .color(colors.toast_err_fg)
                                .small(),
                        );
                    }
                }
            });
        });
        let ok = entry.is_ok();
        let has_photo = self.editor.photo().src_path.is_some();
        let busy = self.workspace.busy();
        ui.add_space(SPACE_XS);
        let running = tr(lang, K_RECIPES_A_TASK_IS_RUNNING_WAIT_FOR_IT_TO_FINISH);
        let needs_photo = || {
            if has_photo {
                running
            } else {
                tr(lang, K_RECIPES_OPEN_A_PHOTO_FIRST)
            }
        };
        ui.horizontal_wrapped(|ui| {
            if primary(ui, ok && has_photo && !busy, tr(lang, K_RECIPES_APPLY_TO_THIS_PHOTO))
                .on_hover_text(tr(lang, K_RECIPES_REPLACES_WHAT_IS_ON_THE_CANVAS_CTRL_Z_UNDOES_IT))
                .on_disabled_hover_text(needs_photo())
                .clicked()
            {
                self.apply_recipe(logic::recipes::RecipeAction::ApplyToOpen(name.to_owned()),);
            }
            if action(ui, ok && has_photo && !busy, tr(lang, K_RECIPES_WRITE_BACK_TO_THIS_RECIPE))
                .on_hover_text(tr(lang, K_RECIPES_SAVES_WHAT_IS_ON_THE_CANVAS_INTO_THIS_ENTRY_KEEP))
                .on_disabled_hover_text(needs_photo())
                .clicked()
            {
                self.apply_recipe(logic::recipes::RecipeAction::UpdateFromOpen(name.to_owned()),);
            }
            if action(ui, ok && !busy, tr(lang, K_RECIPES_RENAME))
                .on_hover_text(tr(lang, K_RECIPES_RENAME_THIS_RECIPE_THE_FILE_MOVES_WITH_IT))
                .clicked()
            {
                self.workspace.rlib_mut().recipe_rename = Some((name.to_string(), name.to_string()));
            }
            let armed = self.workspace.rlib().recipe_delete_armed.as_deref() == Some(name);
            let del = if armed { tr(lang, K_RECIPES_CONFIRM_DELETE) } else { tr(lang, K_RECIPES_DELETE) };
            if action(ui, ok && !busy, del)
                .on_hover_text(tr(
                    lang,
                    K_RECIPES_DELETES_THIS_RECIPE_FILE_FROM_THE_LIBRARY_PHOTOS,
                ))
                .clicked()
            {
                if armed {
                    self.apply_recipe(logic::recipes::RecipeAction::Delete(name.to_owned()),);
                } else {
                    self.workspace.rlib_mut().recipe_delete_armed = Some(name.to_string());
                }
            }
        });
    }


    // What the entry CHANGES, as label/value rows: only the controls that
    // differ from a neutral develop. The numbers are the engine's own.
}

/// One library row: 34 px, the name at the left, its meta at the right edge,
/// a hover wash under the pointer and an accent bar while selected. Painted
/// rather than composed so every row is the same line whatever the name's
/// length (a long stem truncates; the tooltip carries it whole).
fn paint_recipe_row(
    ui: &mut egui::Ui,
    colors: &crate::theme::ThemeColors,
    name: &str,
    meta: &str,
    selected: bool,
    broken: bool,
) -> (egui::Response, bool) {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), RECIPE_ROW_H), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return (resp, false);
    }
    if resp.hovered() {
        // A row is a click target, like a gallery thumbnail (the same cursor
        // language everywhere a click does something).
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect.shrink2(egui::vec2(0.0, 1.0)), RADIUS_SM, colors.sel_bg);
        let bar = egui::Rect::from_min_size(
            rect.left_center() - egui::vec2(0.0, 10.0),
            egui::vec2(2.0, 20.0),
        );
        painter.rect_filled(bar, 1.0, colors.accent_fill);
    } else if resp.hovered() {
        // One quiet step off the panel: the selection is the strong state,
        // the pointer's wash must not out-shout it.
        painter.rect_filled(rect.shrink2(egui::vec2(0.0, 1.0)), RADIUS_SM, colors.card_fill);
    }
    let font = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let weak = if broken { colors.toast_err_fg } else { ui.visuals().weak_text_color() };
    let meta_galley = painter.layout_no_wrap(meta.to_owned(), small, weak);
    let meta_w = meta_galley.size().x;
    painter.galley(
        rect.right_center() - egui::vec2(12.0 + meta_w, meta_galley.size().y / 2.0),
        meta_galley,
        weak,
    );
    let name_w = (rect.width() - 24.0 - meta_w - 16.0).max(40.0);
    let name_color = if selected {
        colors.accent_text
    } else if broken {
        weak
    } else {
        ui.visuals().text_color()
    };
    let shown = truncate_to(ui, name, font.clone(), name_w);
    let truncated = shown != name;
    painter.text(
        rect.left_center() + egui::vec2(14.0, 0.0),
        egui::Align2::LEFT_CENTER,
        shown,
        font,
        name_color,
    );
    (resp, truncated)
}

/// `text` cut to `max_w` with an ellipsis (egui 0.29 has no overflow
/// character on a laid-out galley, so this walks the tail off and re-measures;
/// the galleys are cached, and a row name is a few dozen characters).
fn truncate_to(ui: &egui::Ui, text: &str, font: egui::FontId, max_w: f32) -> String {
    let width = |s: &str| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::PLACEHOLDER).size().x
        })
    };
    if width(text) <= max_w {
        return text.to_owned();
    }
    let mut cut = text.to_owned();
    while !cut.is_empty() {
        cut.pop();
        let candidate = format!("{}…", cut.trim_end());
        if width(&candidate) <= max_w {
            return candidate;
        }
    }
    "…".to_owned()
}

/// The one row height of the library list.
const RECIPE_ROW_H: f32 = 34.0;


/// How old a library file is, in words: the file stamp measured against now
/// (no date crate in the tree, and an elapsed span needs no time zone).
fn age_text(lang: Lang, t: std::time::SystemTime) -> Option<String> {
    let days = t.elapsed().ok()?.as_secs() / 86_400;
    Some(if days == 0 {
        tr(lang, K_RECIPES_TODAY).to_string()
    } else if days == 1 {
        tr(lang, K_RECIPES_YESTERDAY).to_string()
    } else if days < 30 {
        trf(lang, K_RECIPES_DAYS_AGO, &[("n", &days.to_string())])
    } else if days < 365 {
        trf(lang, K_RECIPES_MONTHS_AGO, &[("n", &(days / 30).to_string())])
    } else {
        trf(lang, K_RECIPES_YEARS_AGO, &[("n", &(days / 365).to_string())])
    })
}

pub(crate) fn recipe_facts(lang: Lang, r: &EditRecipe) -> Vec<(String, String)> {
    let d = EditRecipe::default();
    let mut out: Vec<(String, String)> = Vec::new();
    if (r.exposure_ev - d.exposure_ev).abs() > 1e-4 {
        out.push((tr(lang, K_DEVELOP_EXPOSURE).to_string(), format!("{:+.2}", r.exposure_ev)));
    }
    for (label, v, neutral) in [
        ("Contrast", r.contrast, d.contrast),
        ("Highlights", r.highlights, d.highlights),
        ("Shadows", r.shadows, d.shadows),
        ("Whites", r.whites, d.whites),
        ("Blacks", r.blacks, d.blacks),
        ("Tint", r.tint, d.tint),
        ("Vibrance", r.vibrance, d.vibrance),
        ("Saturation", r.saturation, d.saturation),
        ("Texture", r.texture, d.texture),
        ("Clarity", r.clarity, d.clarity),
        ("Dehaze", r.dehaze, d.dehaze),
        ("Sharpening", r.sharpening, d.sharpening),
        ("Noise Reduction", r.noise_reduction, d.noise_reduction),
    ] {
        if (v - neutral).abs() > 0.05 {
            out.push((tr(lang, label).to_string(), format!("{v:+.0}")));
        }
    }
    if let Some(k) = r.temperature_k.filter(|k| *k > 0.0) {
        out.push((tr(lang, K_RECIPES_TEMPERATURE).to_string(), format!("{k:.0} K")));
    }
    let points = r.tone_curve.len() + r.red_curve.len() + r.green_curve.len() + r.blue_curve.len();
    if points > 0 {
        out.push((
            tr(lang, K_DEVELOP_CURVE).to_string(),
            trf(lang, K_RECIPES_POINTS, &[("n", &points.to_string())]),
        ));
    }
    if r.convert_to_grayscale {
        out.push((tr(lang, K_NORMAL_BLACK_WHITE).to_string(), "✓".to_string()));
    }
    if r.hsl != d.hsl {
        out.push((tr(lang, K_DEVELOP_COLOR_MIXER_HSL).to_string(), "✓".to_string()));
    }
    if r.color_grade != d.color_grade {
        out.push((tr(lang, K_DEVELOP_COLOR_GRADING).to_string(), "✓".to_string()));
    }
    if !r.point_colors.is_empty() {
        out.push((tr(lang, K_DEVELOP_POINT_COLOR).to_string(), r.point_colors.len().to_string()));
    }
    if !r.masks.is_empty() {
        out.push((tr(lang, K_RECIPES_MASKS__2).to_string(), r.masks.len().to_string()));
    }
    out
}

#[cfg(test)]
mod age_text_tests {
    use super::age_text;
    use crate::Lang;
    use std::time::{Duration, SystemTime};

    #[test]
    fn a_file_stamp_reads_as_an_elapsed_span() {
        let now = SystemTime::now();
        assert_eq!(age_text(Lang::En, now - Duration::from_secs(60)).as_deref(), Some("today"));
        assert_eq!(
            age_text(Lang::En, now - Duration::from_secs(3 * 86_400)).as_deref(),
            Some("3 days ago")
        );
        assert_eq!(
            age_text(Lang::En, now - Duration::from_secs(60 * 86_400)).as_deref(),
            Some("2 months ago")
        );
        assert_eq!(age_text(Lang::En, now + Duration::from_secs(60)), None, "a future stamp says nothing");
    }
}
