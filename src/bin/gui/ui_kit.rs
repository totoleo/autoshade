//! The GUI's own widget vocabulary: adapters that keep framework types out of
//! the panels. `Text` is the styling builder panels speak; the paint-side
//! widgets (histogram, curve editor, grade wheel) stay their own modules.
//!
//! The vocabulary is deliberately small: it mirrors only the shapes the panels
//! actually use, and a shape that does not fit stays raw at its call site
//! rather than growing this module.

use crate::*;

/// Text styled by role, not by framework widget: a thin builder over the
/// subset of `egui::RichText` the GUI uses. A panel writes
/// `Text::new(tr(...)).weak().small()`; the egui type stays in this module.
#[derive(Clone)]
pub(crate) struct Text(egui::RichText);

impl Text {
    pub(crate) fn new(text: impl Into<String>) -> Self {
        Self(egui::RichText::new(text))
    }

    pub(crate) fn small(self) -> Self {
        Self(self.0.small())
    }

    pub(crate) fn weak(self) -> Self {
        Self(self.0.weak())
    }

    pub(crate) fn strong(self) -> Self {
        Self(self.0.strong())
    }

    pub(crate) fn heading(self) -> Self {
        Self(self.0.heading())
    }

    pub(crate) fn monospace(self) -> Self {
        Self(self.0.monospace())
    }

    pub(crate) fn italics(self) -> Self {
        Self(self.0.italics())
    }

    pub(crate) fn extra_letter_spacing(self, px: f32) -> Self {
        Self(self.0.extra_letter_spacing(px))
    }

    pub(crate) fn size(self, px: f32) -> Self {
        Self(self.0.size(px))
    }

    pub(crate) fn color(self, color: egui::Color32) -> Self {
        Self(self.0.color(color))
    }
}

impl From<Text> for egui::WidgetText {
    fn from(text: Text) -> Self {
        text.0.into()
    }
}
