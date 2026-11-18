//! The form data-entry vocabulary: one single-line field, and the
//! label-plus-field row with the walk that renders a table of rows.
//!
//! The settings panel's simple rows are data here; a row that needs bespoke
//! layout (a button beside the field, a hint that also drives a side effect)
//! stays hand-drawn at its site, and that is on purpose — the descriptor covers
//! what repeats, not everything that exists.

use crate::*;

/// One single-line text field: `hint` shows while the buffer is blank,
/// `secret` masks the input (the API keys), `width` caps it when given.
pub(crate) fn field(
    ui: &mut egui::Ui,
    buffer: &mut String,
    hint: Option<&str>,
    secret: bool,
    width: Option<f32>,
) -> egui::Response {
    let mut edit = egui::TextEdit::singleline(buffer);
    if let Some(hint) = hint {
        edit = edit.hint_text(hint);
    }
    if secret {
        edit = edit.password(true);
    }
    if let Some(width) = width {
        edit = edit.desired_width(width);
    }
    ui.add(edit)
}

/// How wide a row's field grows.
pub(crate) enum FieldWidth {
    /// `FIELD_W_MAX` capped by the room left after `reserve` px.
    Reserving(f32),
}

impl FieldWidth {
    fn desired(&self, ui: &egui::Ui) -> f32 {
        match *self {
            Self::Reserving(reserve) => FIELD_W_MAX.min(ui.avail_width() - reserve).max(80.0),
        }
    }
}

/// One `label: field` row.
pub(crate) struct TextRow<'a> {
    pub(crate) label: &'static str,
    pub(crate) buffer: &'a mut String,
    pub(crate) width: FieldWidth,
}

impl<'a> TextRow<'a> {
    /// A row whose field leaves `reserve` px for whatever sits beside it.
    pub(crate) fn reserving(label: &'static str, buffer: &'a mut String, reserve: f32) -> Self {
        Self {
            label,
            buffer,
            width: FieldWidth::Reserving(reserve),
        }
    }
}

/// Draw `rows` in order, a label and its field each. Returns true when any
/// field changed this frame.
pub(crate) fn text_rows(ui: &mut egui::Ui, lang: Lang, rows: &mut [TextRow<'_>]) -> bool {
    let mut changed = false;
    for row in rows.iter_mut() {
        ui.row(|ui| {
            ui.label(tr(lang, row.label));
            let width = row.width.desired(ui);
            changed |= field(ui, row.buffer, None, false, Some(width)).changed();
        });
    }
    changed
}
